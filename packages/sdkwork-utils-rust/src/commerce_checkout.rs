//! Shared commerce checkout/cashier URL helpers for order and payment capabilities.

/// H5 app origin hosting the order cashier route.
///
/// `SDKWORK_COMMERCE_CASHIER_BASE_URL` may override it per deployment; it
/// must carry only the origin (no trailing cashier path), because
/// `build_commerce_cashier_url` appends the canonical H5 cashier route.
const DEFAULT_CASHIER_BASE_URL: &str = "https://im.sdkwork.com";

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
pub fn commerce_cashier_scene(order_subject: Option<&str>) -> &'static str {
    match order_subject
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_ascii_lowercase)
        .as_deref()
    {
        Some("points_recharge") => "recharge",
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
    format!(
        "{}/cashier/{}?scene={}&outTradeNo={}",
        base, order_id, scene, out_trade_no
    )
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
    fn cashier_scene_maps_points_recharge() {
        assert_eq!(commerce_cashier_scene(Some("points_recharge")), "recharge");
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
    fn alipay_trade_success_is_success() {
        assert!(commerce_webhook_payment_status_is_success(Some(
            "TRADE_SUCCESS"
        )));
    }
}
