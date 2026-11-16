//! Shared commerce checkout/cashier URL helpers for order and payment capabilities.

/// H5 app origin hosting the order cashier route.
///
/// `SDKWORK_COMMERCE_CASHIER_BASE_URL` may override it per deployment.
/// The configured value must carry only the origin (no trailing cashier path),
/// because `build_commerce_cashier_url` appends the canonical H5 cashier route.
/// The platform default is the unified SDKWork platform host, which serves the
/// shared cashier route at `/cashier/{order_id}`.
const DEFAULT_CASHIER_BASE_URL: &str = "https://platform.sdkwork.com";

/// Resolve the H5 cashier base origin from `SDKWORK_COMMERCE_CASHIER_BASE_URL`
/// or the platform default.
pub fn commerce_cashier_base_url() -> String {
    std::env::var("SDKWORK_COMMERCE_CASHIER_BASE_URL")
        .ok()
        .map(|value| value.trim().trim_end_matches('/').to_owned())
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| DEFAULT_CASHIER_BASE_URL.to_owned())
}

/// Map `commerce_order.subject` to a cashier scene query parameter.
///
/// Every account-value recharge subject renders the recharge scene so the
/// cashier shows the purchase summary and pay action, not the physical
/// checkout flow. `points_recharge` (Membership Points) and the Token Bank
/// subjects share the scene but remain distinct assets on the ledger.
pub fn commerce_cashier_scene(order_subject: Option<&str>) -> &'static str {
    match order_subject
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_ascii_lowercase)
        .as_deref()
    {
        Some("points_recharge")
        | Some("token_bank_recharge")
        | Some("token_bank_plan_purchase")
        | Some("token_bank_plan_renewal")
        | Some("account_recharge_package")
        | Some("coupon_recharge") => "recharge",
        Some("product") | Some("physical") => "checkout",
        Some("virtual_goods") | Some("membership") => "virtual",
        // Published app templates are digital listings: the cashier renders the
        // virtual-goods scene, not the physical checkout scene.
        Some("app_template") | Some("deploy_app_template") => "virtual",
        _ => "checkout",
    }
}

/// Build a cashier deep-link for owner-order payment.
///
/// The URL targets the `sdkwork-im-h5` unified cashier route
/// (`/cashier/{order_id}`) so the QR content matches the mounted H5 cashier
/// exactly (history-mode routing, no hash fragment). `orderId` carries the
/// order primary key, which is the key the H5 cashier page uses with
/// `orders.retrieve(orderId)`.
pub fn build_commerce_cashier_url(scene: &str, order_id: &str, out_trade_no: &str) -> String {
    build_commerce_cashier_url_with_base(scene, order_id, out_trade_no, None)
}

/// Build a cashier deep-link against an explicit base origin.
///
/// `base_url` is the admin-configured H5 payment domain (`commerce_payment_domain_setting`),
/// resolved by the payment capability. Callers pass the configured origin so the
/// generated QR content follows operator configuration; `None` falls back to
/// `commerce_cashier_base_url()` (env override, then platform default).
///
/// The base must be an origin only (`scheme://host[:port]`); a trailing slash is
/// tolerated because operators routinely paste `https://h5.example.com/`.
pub fn build_commerce_cashier_url_with_base(
    scene: &str,
    order_id: &str,
    out_trade_no: &str,
    base_url: Option<&str>,
) -> String {
    let base = base_url
        .map(str::trim)
        .map(|value| value.trim_end_matches('/'))
        .filter(|value| !value.is_empty())
        .map(str::to_owned)
        .unwrap_or_else(commerce_cashier_base_url);
    // The legacy link carries an order id and a merchant trade number. Both are
    // token components: encode them so an out-trade number containing `&`, `#`,
    // `?` or whitespace cannot truncate or reshape the generated QR URL.
    format!(
        "{}/cashier/{}?scene={}&outTradeNo={}",
        base,
        encode_url_component(order_id),
        encode_url_component(scene),
        encode_url_component(out_trade_no)
    )
}

/// Percent-encodes a URL path/query component using the RFC 3986 unreserved set.
///
/// Kept local (no extra dependency) because the cashier URL is the only place in
/// this crate that builds query strings from caller-supplied text.
fn encode_url_component(value: &str) -> String {
    let mut encoded = String::with_capacity(value.len());
    for byte in value.as_bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' => {
                encoded.push(*byte as char);
            }
            _ => {
                encoded.push('%');
                encoded.push_str(&format!("{byte:02X}"));
            }
        }
    }
    encoded
}

/// Build the anonymous cashier deep-link for a signed capability token.
///
/// The URL carries the token as its path segment and **no** plaintext order id or
/// trade number. An anonymous cashier must not expose an enumerable key, and every
/// fact the page needs (scene, amount, expiry) is bound inside the signed token
/// instead of in query parameters the client could tamper with.
///
/// Returns `None` when `token` is blank: a tokenless URL would degrade to the
/// login-required cashier entry, which callers must handle explicitly rather than
/// emit silently.
pub fn build_commerce_cashier_token_url(token: &str) -> Option<String> {
    build_commerce_cashier_token_url_with_base(token, None)
}

/// Build the anonymous cashier deep-link against an explicit base origin.
///
/// `base_url` is the admin-configured H5 payment domain; `None` falls back to
/// [`commerce_cashier_base_url`]. A trailing slash is tolerated because operators
/// routinely paste `https://h5.example.com/`.
pub fn build_commerce_cashier_token_url_with_base(
    token: &str,
    base_url: Option<&str>,
) -> Option<String> {
    let token = token.trim();
    if token.is_empty() {
        return None;
    }
    let base = base_url
        .map(str::trim)
        .map(|value| value.trim_end_matches('/'))
        .filter(|value| !value.is_empty())
        .map(str::to_owned)
        .unwrap_or_else(commerce_cashier_base_url);
    Some(format!(
        "{base}/cashier/{}",
        encode_url_component(token)
    ))
}

/// Returns true when a normalized provider webhook status indicates payment success.
pub fn commerce_webhook_payment_status_is_success(status: Option<&str>) -> bool {
    matches!(
        status
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_ascii_lowercase)
            .as_deref(),
        Some("succeeded" | "success" | "paid" | "trade_success" | "payment.succeeded")
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cashier_default_base_is_the_unified_platform_host() {
        assert_eq!("https://platform.sdkwork.com", DEFAULT_CASHIER_BASE_URL);
    }

    #[test]
    fn cashier_scene_maps_points_recharge() {
        assert_eq!(commerce_cashier_scene(Some("points_recharge")), "recharge");
    }

    #[test]
    fn cashier_scene_maps_every_recharge_subject_to_recharge() {
        for subject in [
            "points_recharge",
            "token_bank_recharge",
            "token_bank_plan_purchase",
            "token_bank_plan_renewal",
            "account_recharge_package",
            "coupon_recharge",
            "  TOKEN_BANK_RECHARGE  ",
        ] {
            assert_eq!(
                commerce_cashier_scene(Some(subject)),
                "recharge",
                "subject {subject:?} must render the recharge cashier scene"
            );
        }
    }

    #[test]
    fn cashier_scene_maps_a_published_app_template() {
        assert_eq!(commerce_cashier_scene(Some("app_template")), "virtual");
        assert_eq!(
            commerce_cashier_scene(Some("deploy_app_template")),
            "virtual"
        );
    }

    #[test]
    fn build_cashier_url_uses_h5_cashier_route() {
        let url = build_commerce_cashier_url("recharge", "ORD-1", "OT-1");
        assert!(url.contains("/cashier/ORD-1"));
        assert!(url.contains("scene=recharge"));
        assert!(url.contains("outTradeNo=OT-1"));
        assert!(!url.contains('#'));
    }

    #[test]
    fn build_cashier_url_honors_configured_h5_base() {
        let url = build_commerce_cashier_url_with_base(
            "recharge",
            "ORD-1",
            "OT-1",
            Some("https://h5.example.com/"),
        );
        assert_eq!(
            "https://h5.example.com/cashier/ORD-1?scene=recharge&outTradeNo=OT-1",
            url
        );
    }

    #[test]
    fn build_cashier_url_ignores_a_blank_configured_base() {
        let url = build_commerce_cashier_url_with_base("recharge", "ORD-1", "OT-1", Some("  "));
        assert!(url.contains("/cashier/ORD-1"));
        assert!(url.starts_with("https://"));
    }

    #[test]
    fn legacy_cashier_url_percent_encodes_trade_numbers_and_order_ids() {
        let url = build_commerce_cashier_url_with_base(
            "virtual",
            "ORD/1",
            "OT&1 #2?x=1",
            Some("https://h5.example.com/"),
        );
        // Query separators must not survive inside the bound trade number, and
        // the path segment must not introduce a second path component.
        assert_eq!(
            "https://h5.example.com/cashier/ORD%2F1?scene=virtual&outTradeNo=OT%261%20%232%3Fx%3D1",
            url
        );
    }

    #[test]
    fn legacy_cashier_url_keeps_unreserved_trade_numbers_readable() {
        let url = build_commerce_cashier_url_with_base(
            "recharge",
            "ORD-1",
            "MEMBERSHIP88d35e114d97f6e5",
            Some("https://h5.example.com"),
        );
        assert_eq!(
            "https://h5.example.com/cashier/ORD-1?scene=recharge&outTradeNo=MEMBERSHIP88d35e114d97f6e5",
            url
        );
    }

    #[test]
    fn alipay_trade_success_is_success() {
        assert!(commerce_webhook_payment_status_is_success(Some(
            "TRADE_SUCCESS"
        )));
    }

    #[test]
    fn token_cashier_url_carries_only_the_token() {
        let url = build_commerce_cashier_token_url_with_base(
            "v1.eyJvcmRlcklkIjoiT1JELTEifQ.sig",
            Some("https://h5.example.com/"),
        )
        .expect("token url");
        assert_eq!(
            "https://h5.example.com/cashier/v1.eyJvcmRlcklkIjoiT1JELTEifQ.sig",
            url
        );
        // The anonymous entry must never leak an enumerable order key or the
        // merchant trade number into the URL.
        assert!(!url.contains("ORD-1"));
        assert!(!url.contains("scene="));
        assert!(!url.contains("outTradeNo="));
    }

    #[test]
    fn token_cashier_url_encodes_a_non_base64url_path_segment() {
        // Current tokens are base64url and unchanged by the encoder; a future
        // token shape with a path separator must still stay one path segment.
        let url = build_commerce_cashier_token_url_with_base(
            "v1.a/b",
            Some("https://h5.example.com"),
        )
        .expect("token url");
        assert_eq!("https://h5.example.com/cashier/v1.a%2Fb", url);
    }

    #[test]
    fn token_cashier_url_falls_back_to_the_deployment_base() {
        let url = build_commerce_cashier_token_url("v1.payload.sig").expect("token url");
        assert!(url.starts_with("https://"));
        assert!(url.ends_with("/cashier/v1.payload.sig"));
    }

    #[test]
    fn token_cashier_url_rejects_a_blank_token() {
        assert_eq!(build_commerce_cashier_token_url("   "), None);
        assert_eq!(
            build_commerce_cashier_token_url_with_base("", Some("https://h5.example.com")),
            None
        );
    }
}
