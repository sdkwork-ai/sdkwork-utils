//! Machine-service credentials for internal-api and backend-agent HTTP surfaces.
//!
//! `APP_SDK_INTEGRATION_SPEC.md` §5.2.1 handles *user-scoped* hops through
//! [`crate::caller_identity`]. This module covers the other side of the same
//! contract: a background/worker caller with no end-user request must present an
//! **explicitly declared service identity**.
//!
//! The runtime authentication chain for the two machine surfaces is:
//!
//! - internal-api: `X-SDKWork-Ingress-Token` + `Access-Token`
//!   (`RouteAuth::IngressToken`);
//! - backend-agent: `X-SDKWork-Agent-Token` + `Access-Token`
//!   (`RouteAuth::AgentToken`);
//!
//! where the service token proves the machine identity and the `Access-Token`
//! supplies the IAM application/tenant scope the principal resolves against. The
//! two headers are a pair: a generated SDK's default `X-Api-Key` surface is
//! **rejected** by both profiles as credential-profile contamination, and a
//! service token without the access token fails `missing-access-token`.
//!
//! A deployment must therefore be configured with both values. Comparing the
//! pair before dispatch and failing closed with
//! [`missing_service_credentials`] keeps a half-configured deployment from
//! dispatching an unauthenticated request that the destination reports as a
//! generic 400/401.

/// Header carrying the internal-api service token.
pub const SERVICE_INGRESS_TOKEN_HEADER: &str = "X-SDKWork-Ingress-Token";
/// Header carrying the backend-agent service token.
pub const SERVICE_AGENT_TOKEN_HEADER: &str = "X-SDKWork-Agent-Token";
/// Header carrying the IAM access token that scopes a machine caller.
pub const SERVICE_ACCESS_TOKEN_HEADER: &str = "Access-Token";

/// Which machine surface a credential pair authenticates against.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ServiceTokenKind {
    /// `X-SDKWork-Ingress-Token` (internal-api).
    Ingress,
    /// `X-SDKWork-Agent-Token` (backend-agent routes).
    Agent,
}

impl ServiceTokenKind {
    /// The dedicated service-token header for this surface.
    #[must_use]
    pub fn header_name(self) -> &'static str {
        match self {
            Self::Ingress => SERVICE_INGRESS_TOKEN_HEADER,
            Self::Agent => SERVICE_AGENT_TOKEN_HEADER,
        }
    }
}

/// The declared service identity of one machine deployment.
///
/// Both halves are required: the service token without the access token cannot
/// pass the runtime's tenant-isolation stage, and the access token without the
/// service token cannot prove the machine identity.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct ServiceCredentials {
    service_token: String,
    access_token: String,
}

impl ServiceCredentials {
    /// Builds the pair, trimming both values.
    #[must_use]
    pub fn new(service_token: impl Into<String>, access_token: impl Into<String>) -> Self {
        Self {
            service_token: service_token.into().trim().to_owned(),
            access_token: access_token.into().trim().to_owned(),
        }
    }

    /// Whether both halves of the declared service identity are present.
    #[must_use]
    pub fn is_complete(&self) -> bool {
        !self.service_token.is_empty() && !self.access_token.is_empty()
    }

    /// The exact outbound headers for the given machine surface.
    ///
    /// Callers must check [`Self::is_complete`] first; this returns the rendered
    /// pair regardless so a caller can log or assert it.
    #[must_use]
    pub fn wire_headers(&self, kind: ServiceTokenKind) -> Vec<(&'static str, String)> {
        vec![
            (kind.header_name(), self.service_token.clone()),
            (SERVICE_ACCESS_TOKEN_HEADER, self.access_token.clone()),
        ]
    }
}

/// Diagnostic for a machine deployment that is missing part of its declared
/// service identity.
#[must_use]
pub fn missing_service_credentials(
    surface: &str,
    service_token_env: &str,
    access_token_env: &str,
) -> String {
    format!(
        "{surface} requires a declared service identity: configure both {service_token_env} \
         (service token) and {access_token_env} (IAM application access token) before enabling \
         this split deployment"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_complete_pair_renders_both_profile_headers() {
        let credentials = ServiceCredentials::new(" ingress-token ", " access-token ");
        assert!(credentials.is_complete());
        assert_eq!(
            credentials.wire_headers(ServiceTokenKind::Ingress),
            vec![
                (SERVICE_INGRESS_TOKEN_HEADER, "ingress-token".to_owned()),
                (SERVICE_ACCESS_TOKEN_HEADER, "access-token".to_owned()),
            ]
        );
        assert_eq!(
            credentials.wire_headers(ServiceTokenKind::Agent)[0].0,
            SERVICE_AGENT_TOKEN_HEADER
        );
    }

    #[test]
    fn either_half_missing_is_incomplete_and_never_rendered_as_a_credential() {
        assert!(!ServiceCredentials::new("", "access").is_complete());
        assert!(!ServiceCredentials::new("ingress", "  ").is_complete());
        assert!(!ServiceCredentials::default().is_complete());
    }

    #[test]
    fn the_diagnostic_names_the_surface_and_both_settings() {
        let message = missing_service_credentials(
            "sdkwork-webserver internal-api",
            "SDKWORK_DEPLOY_WEB_INTERNAL_API_INGRESS_TOKEN_FILE",
            "SDKWORK_DEPLOY_WEB_INTERNAL_API_ACCESS_TOKEN_FILE",
        );
        assert!(message.contains("sdkwork-webserver internal-api"));
        assert!(message.contains("SDKWORK_DEPLOY_WEB_INTERNAL_API_INGRESS_TOKEN_FILE"));
        assert!(message.contains("SDKWORK_DEPLOY_WEB_INTERNAL_API_ACCESS_TOKEN_FILE"));
    }
}
