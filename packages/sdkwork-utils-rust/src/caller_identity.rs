//! Caller-identity propagation for backend HTTP SDK calls.
//!
//! `APP_SDK_INTEGRATION_SPEC.md` §5.2.1 makes caller identity a property of the
//! *call*, not of the process: a backend module that calls another SDKWork
//! surface on behalf of an end user `MUST` carry that user's authenticated
//! identity (the dual tokens — `Authorization: Bearer <auth token>` plus
//! `Access-Token: <session token>`) or the resolved typed request context. It
//! `MUST NOT` substitute process credentials, mint credentials, or reuse one
//! request's token for another caller.
//!
//! Every registry-resolved HTTP hop (the default production path, see
//! `service_base_url`) lands on the same ingress authentication stack, so the
//! wire shape is identical whether the target came from the service registry,
//! an authored topology value, or an explicit override. This module is that one
//! place: it normalizes inbound headers into a typed carrier, renders the exact
//! outbound headers, and names the missing-identity diagnostic so an operator
//! can tell "no caller identity was present" apart from "the destination
//! rejected the identity".
//!
//! Tenant and organization scope are not separate headers: the destination
//! resolves them *from the credential* (`API_SPEC.md` §10.2 / §10.4). Carrying
//! the caller's tokens is therefore what keeps user and multi-tenant context
//! intact across an internal hop; trusting caller-supplied tenant headers
//! instead would be a tenant-isolation defect.

/// Outbound authorization header for the caller's auth token.
pub const AUTHORIZATION_HEADER: &str = "Authorization";
/// Outbound header for the caller's application session token.
pub const ACCESS_TOKEN_HEADER: &str = "Access-Token";

/// Which half of the dual-token pair a caller-identity application is setting.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CallerTokenKind {
    /// OAuth/JWT bearer (`Authorization: Bearer <token>`).
    Auth,
    /// Application session token (`Access-Token: <token>`).
    Access,
}

/// The caller's authenticated dual credentials for one request scope.
///
/// The `Debug` implementation redacts both tokens: a credential carrier is
/// frequently embedded in service contexts and task spans, and neither may
/// write raw token material into logs or panic messages.
#[derive(Clone, Default, Eq, PartialEq)]
pub struct CallerCredentials {
    auth_token: Option<String>,
    access_token: Option<String>,
}

impl std::fmt::Debug for CallerCredentials {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("CallerCredentials")
            .field(
                "auth_token",
                &self.auth_token.as_ref().map(|_| "<redacted>"),
            )
            .field(
                "access_token",
                &self.access_token.as_ref().map(|_| "<redacted>"),
            )
            .finish()
    }
}

impl CallerCredentials {
    /// Builds the carrier from already-extracted raw values, applying the
    /// canonical normalization (trim, blank-is-absent, `Bearer ` stripped from
    /// the auth token).
    #[must_use]
    pub fn new(auth_token: Option<String>, access_token: Option<String>) -> Self {
        Self {
            auth_token: normalize_auth_token(auth_token),
            access_token: normalize_token(access_token),
        }
    }

    /// Reads the dual credentials from inbound request headers.
    ///
    /// `get` receives the lowercase canonical header names
    /// ([`AUTHORIZATION_HEADER`] and [`ACCESS_TOKEN_HEADER`] lowercased) and
    /// returns the raw header value. Callers pass their framework's
    /// case-insensitive header lookup (for example
    /// `|name| headers.get(name).and_then(|value| value.to_str().ok()).map(str::to_owned)`).
    #[must_use]
    pub fn from_header_lookup(mut get: impl FnMut(&str) -> Option<String>) -> Self {
        let authorization = get("authorization");
        let access_token = get("access-token");
        Self::new(authorization, access_token)
    }

    /// The caller's auth token (OAuth/JWT bearer), when present.
    #[must_use]
    pub fn auth_token(&self) -> Option<&str> {
        self.auth_token.as_deref()
    }

    /// The caller's application session token, when present.
    #[must_use]
    pub fn access_token(&self) -> Option<&str> {
        self.access_token.as_deref()
    }

    /// Whether the request carried no caller identity at all.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.auth_token.is_none() && self.access_token.is_none()
    }

    /// Exact outbound headers for a generated SDK client's header surface.
    ///
    /// A generated client accepts headers either through its typed setters
    /// ([`Self::apply_typed`]) or through `set_header` ([`Self::apply`]); both
    /// render the same wire contract. Only the headers the caller actually
    /// presented are returned — never a synthesized or widened credential.
    #[must_use]
    pub fn wire_headers(&self) -> Vec<(&'static str, String)> {
        let mut headers = Vec::with_capacity(2);
        if let Some(auth_token) = self.auth_token() {
            headers.push((AUTHORIZATION_HEADER, format!("Bearer {auth_token}")));
        }
        if let Some(access_token) = self.access_token() {
            headers.push((ACCESS_TOKEN_HEADER, access_token.to_owned()));
        }
        headers
    }

    /// Non-reversible short fingerprint of the credential pair, for client-cache
    /// keys (`APP_SDK_INTEGRATION_SPEC.md` §5.4: cache by
    /// `(base URL, credential fingerprint)`). Two calls from the same caller
    /// share a pooled client; callers with different credentials never do.
    #[must_use]
    pub fn fingerprint(&self) -> String {
        use sha2::{Digest, Sha256};

        let mut hasher = Sha256::new();
        hasher.update(self.auth_token.as_deref().unwrap_or_default().as_bytes());
        hasher.update(b"\0");
        hasher.update(self.access_token.as_deref().unwrap_or_default().as_bytes());
        let digest = hasher.finalize();
        digest
            .iter()
            .take(8)
            .map(|byte| format!("{byte:02x}"))
            .collect()
    }

    /// Applies the caller headers through a generated client's `set_header`
    /// surface.
    pub fn apply(&self, mut set_header: impl FnMut(&'static str, String)) {
        for (name, value) in self.wire_headers() {
            set_header(name, value);
        }
    }

    /// Applies the caller headers through a generated client's typed setters.
    ///
    /// The access token is applied first: several generated clients clean up a
    /// stale API-key header inside `set_access_token`, while `set_auth_token`
    /// owns the `Authorization` bearer. This order is a no-op for clients whose
    /// setters are independent, so it is safe everywhere.
    pub fn apply_typed(&self, mut set: impl FnMut(CallerTokenKind, &str)) {
        if let Some(access_token) = self.access_token() {
            set(CallerTokenKind::Access, access_token);
        }
        if let Some(auth_token) = self.auth_token() {
            set(CallerTokenKind::Auth, auth_token);
        }
    }
}

/// Diagnostic for a call refused locally because the request carried no caller
/// identity (`APP_SDK_INTEGRATION_SPEC.md` §5.2.1: an identity failure MUST name
/// the destination surface and distinguish a missing identity from a rejected
/// one).
#[must_use]
pub fn missing_caller_identity(surface: &str) -> String {
    format!(
        "{surface} call requires the caller's authenticated identity, but the request carried \
         neither an Authorization bearer nor an Access-Token"
    )
}

fn normalize_auth_token(value: Option<String>) -> Option<String> {
    normalize_token(value)
        .map(|token| {
            let mut parts = token.splitn(2, char::is_whitespace);
            let first = parts.next().unwrap_or_default();
            if first.eq_ignore_ascii_case("bearer") {
                parts.next().map(str::trim).unwrap_or_default().to_owned()
            } else {
                token
            }
        })
        .filter(|token| !token.is_empty())
}

fn normalize_token(value: Option<String>) -> Option<String> {
    value
        .map(|token| token.trim().to_owned())
        .filter(|token| !token.is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn inbound_headers_normalize_into_the_typed_carrier() {
        let credentials = CallerCredentials::from_header_lookup(|name| match name {
            "authorization" => Some("  Bearer auth-token-value  ".to_owned()),
            "access-token" => Some("access-token-value".to_owned()),
            _ => None,
        });
        assert_eq!(credentials.auth_token(), Some("auth-token-value"));
        assert_eq!(credentials.access_token(), Some("access-token-value"));
        assert!(!credentials.is_empty());
    }

    #[test]
    fn blank_and_scheme_only_values_are_absent_not_synthesized() {
        let credentials =
            CallerCredentials::new(Some("Bearer   ".to_owned()), Some("   ".to_owned()));
        assert!(
            credentials.is_empty(),
            "blank credentials must not be forwarded"
        );
    }

    #[test]
    fn wire_headers_render_the_dual_token_contract() {
        let credentials = CallerCredentials::new(
            Some("auth-token".to_owned()),
            Some("access-token".to_owned()),
        );
        assert_eq!(
            credentials.wire_headers(),
            vec![
                (AUTHORIZATION_HEADER, "Bearer auth-token".to_owned()),
                (ACCESS_TOKEN_HEADER, "access-token".to_owned()),
            ]
        );
    }

    #[test]
    fn access_token_only_callers_forward_only_that_header() {
        let credentials = CallerCredentials::new(None, Some("access-token".to_owned()));
        assert_eq!(
            credentials.wire_headers(),
            vec![(ACCESS_TOKEN_HEADER, "access-token".to_owned())]
        );
    }

    #[test]
    fn typed_setters_apply_access_first_then_auth() {
        let credentials = CallerCredentials::new(
            Some("auth-token".to_owned()),
            Some("access-token".to_owned()),
        );
        let mut applied = Vec::new();
        credentials.apply_typed(|kind, token| {
            applied.push(match kind {
                CallerTokenKind::Auth => ("auth", token.to_owned()),
                CallerTokenKind::Access => ("access", token.to_owned()),
            });
        });
        assert_eq!(
            applied,
            vec![
                ("access", "access-token".to_owned()),
                ("auth", "auth-token".to_owned()),
            ]
        );
    }

    #[test]
    fn missing_identity_diagnostic_names_the_surface() {
        let message = missing_caller_identity("sdkwork-drive");
        assert!(message.starts_with("sdkwork-drive call requires"));
        assert!(message.contains("neither an Authorization bearer nor an Access-Token"));
    }

    #[test]
    fn debug_output_never_prints_raw_tokens() {
        let credentials = CallerCredentials::new(
            Some("secret-auth-token".to_owned()),
            Some("secret-access-token".to_owned()),
        );
        let rendered = format!("{credentials:?}");
        assert!(!rendered.contains("secret-auth-token"));
        assert!(!rendered.contains("secret-access-token"));
        assert!(rendered.contains("<redacted>"));
    }

    #[test]
    fn fingerprint_is_stable_per_pair_and_differs_across_callers() {
        let a = CallerCredentials::new(Some("auth-a".to_owned()), Some("access-a".to_owned()));
        let a_again =
            CallerCredentials::new(Some("auth-a".to_owned()), Some("access-a".to_owned()));
        let b = CallerCredentials::new(Some("auth-b".to_owned()), Some("access-a".to_owned()));
        assert_eq!(a.fingerprint(), a_again.fingerprint());
        assert_ne!(a.fingerprint(), b.fingerprint());
        assert_eq!(a.fingerprint().len(), 16);
        assert!(!a.fingerprint().contains("auth-a"));
    }
}
