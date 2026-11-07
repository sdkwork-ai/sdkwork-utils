//! Service base-URL resolution for backend Rust SDK consumers.
//!
//! One cohesive component owns the whole decision so no caller hand-rolls an
//! origin. Before this module every consumer re-derived a base URL from the
//! environment on each call, and several implementations fabricated
//! `http://127.0.0.1:{port}` from the process' own ingress bind — an
//! outbound-to-self loop. The rule is therefore encoded once, here, and the
//! gate `check-embedded-self-loop.mjs` (APPLICATION_GATEWAY_SPEC §2.3,
//! APP_SDK_INTEGRATION_SPEC §5.2) fails any caller that tries it again.
//!
//! # The two deployment modes
//!
//! Resolution is driven by the deployment profile, never by listener shape:
//!
//! - **`split`** (cloud and separately deployed applications) — the target
//!   surface runs in another process, so a base URL is required. It is taken
//!   from an explicit override or the topology's authored public HTTP URL,
//!   which in production is an absolute domain.
//! - **`embedded`** (standalone, and any composition root that mounts the
//!   surface in-process) — the target surface is linked into *this* process and
//!   consumed through a declared in-process port. There is no base URL to
//!   derive, and deriving one from this process' own bind is prohibited.
//!
//! # Example
//!
//! ```
//! use sdkwork_utils_rust::service_base_url::{resolve, ServiceBaseUrlRequest, ServiceMode};
//!
//! let request = ServiceBaseUrlRequest::new("cloudrouter", ServiceMode::Split)
//!     .with_explicit_override(Some("https://router.sdkwork.com".into()))
//!     .with_authored_public_url(Some("http://127.0.0.1:3905".into()))
//!     .with_split_default("http://127.0.0.1:3900");
//!
//! let resolved = resolve(&request);
//! // The explicit override wins over the authored value.
//! assert_eq!(resolved.base_url(), Some("https://router.sdkwork.com"));
//! ```

use std::sync::{Arc, OnceLock};

/// One HTTP target selected by a service registry.
///
/// `base_url` is an absolute `http(s)` origin. `instance_id` identifies the
/// registered process/port so runtime failures can demote exactly that target
/// instead of every replica of the service.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RegistryHttpTarget {
    /// Registry service name the target was resolved for.
    pub service: String,
    /// Stable instance identity (`host:port` or deployment-scoped id).
    pub instance_id: String,
    /// Absolute `http(s)` base URL, without a trailing slash.
    pub base_url: String,
}

/// Process-wide HTTP service registry consulted by [`resolve`] in split mode.
///
/// The trait lives here, in the dependency-free utility crate, so every backend
/// SDK consumer can resolve through one component. The database-backed
/// implementation is supplied by `sdkwork-discovery-client` and installed once
/// by the composition root. Resolution is synchronous and must read a local
/// cache: it is called on request paths, never from a database round trip.
pub trait HttpServiceRegistry: Send + Sync {
    /// Returns the best currently-healthy target for `service`, or `None` when
    /// the registry has no healthy instance (callers then degrade to the
    /// authored topology value or the documented split default).
    fn resolve_http(&self, service: &str) -> Option<RegistryHttpTarget>;

    /// Reports a transport/protocol failure so the target is demoted for a
    /// bounded cooldown; the next resolution prefers another healthy instance.
    fn report_failure(&self, target: &RegistryHttpTarget, reason: &str);

    /// Reports a successful call so the target's failure streak is reset.
    fn report_success(&self, target: &RegistryHttpTarget);
}

static HTTP_SERVICE_REGISTRY: OnceLock<Arc<dyn HttpServiceRegistry>> = OnceLock::new();

/// Installs the process-wide HTTP service registry.
///
/// Returns `true` when this call won the slot. A second installation is
/// reported as `false` rather than silently replacing the first: one process
/// has one registry view, and a duplicate install means the wiring is wrong.
#[must_use = "a `false` result means an earlier registry was kept"]
pub fn install_http_service_registry(registry: Arc<dyn HttpServiceRegistry>) -> bool {
    HTTP_SERVICE_REGISTRY.set(registry).is_ok()
}

/// The installed process-wide HTTP service registry, when bootstrap installed
/// one.
pub fn installed_http_service_registry() -> Option<&'static Arc<dyn HttpServiceRegistry>> {
    HTTP_SERVICE_REGISTRY.get()
}

/// Returns the registry-selected target for `service`, if a healthy instance is
/// currently cached.
///
/// Consumers that construct a generated HTTP SDK per call use this to replace
/// their configured base URL. `None` means "degrade to the configured value".
pub fn registry_http_target(service: &str) -> Option<RegistryHttpTarget> {
    installed_http_service_registry()?.resolve_http(service)
}

/// Resolves a consumer's base URL: a healthy registry target first, then the
/// caller's configured value.
///
/// The configured value is returned unchanged (minus a trailing slash) when no
/// registry is installed or no healthy instance exists, so a split deployment
/// that has not migrated yet keeps working.
pub fn registry_base_url_or(service: &str, configured: &str) -> String {
    registry_http_target(service)
        .map(|target| target.base_url)
        .unwrap_or_else(|| configured.trim().trim_end_matches('/').to_owned())
}

/// Reports a failure against the target the registry currently selects for
/// `service`.
///
/// Use this from an error mapper that does not carry the resolved base URL. The
/// report is ignored when no registry/target is installed.
pub fn report_current_registry_failure(service: &str, reason: &str) {
    if let Some(registry) = installed_http_service_registry() {
        if let Some(target) = registry.resolve_http(service) {
            registry.report_failure(&target, reason);
        }
    }
}

/// Reports a success against the target the registry currently selects for
/// `service`.
pub fn report_current_registry_success(service: &str) {
    if let Some(registry) = installed_http_service_registry() {
        if let Some(target) = registry.resolve_http(service) {
            registry.report_success(&target);
        }
    }
}

/// Reports the outcome of one SDK call so the registry can demote a failing
/// instance and reset a recovering one.
///
/// The report is ignored when the registry did not select `base_url` (for
/// example the caller used an explicit override or the configured fallback):
/// demoting a target the registry is not routing to would be incorrect.
pub fn report_registry_result(service: &str, base_url: &str, success: bool, reason: &str) {
    let Some(registry) = installed_http_service_registry() else {
        return;
    };
    let Some(target) = registry.resolve_http(service) else {
        return;
    };
    if target.base_url.trim_end_matches('/') != base_url.trim().trim_end_matches('/') {
        return;
    }
    if success {
        registry.report_success(&target);
    } else {
        registry.report_failure(&target, reason);
    }
}

/// Whether the target surface runs in another process or is composed in-process.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ServiceMode {
    /// The target surface runs in a separate process: an HTTP base URL is
    /// required and will be resolved.
    Split,
    /// The target surface is composed into this process: there is no HTTP base
    /// URL, and none may be derived from this process' own listener.
    Embedded,
}

/// Where a resolved base URL came from. Callers surface this in diagnostics so
/// an operator can tell a configured value from a fallback.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BaseUrlSource {
    /// An explicit per-service override environment variable.
    ExplicitOverride,
    /// A healthy instance selected from the process' service registry.
    Registry,
    /// The topology's authored public HTTP URL for the surface.
    AuthoredPublicUrl,
    /// The compiled-in default for a split deployment.
    SplitDefault,
    /// The surface is composed in-process, so no base URL applies.
    EmbeddedInProcess,
}

impl BaseUrlSource {
    /// Stable, machine-readable label for logs and problem details.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::ExplicitOverride => "explicit_override",
            Self::Registry => "registry",
            Self::AuthoredPublicUrl => "authored_public_url",
            Self::SplitDefault => "split_default",
            Self::EmbeddedInProcess => "embedded_in_process",
        }
    }
}

/// The resolved base URL for one service target.
///
/// A `None` base URL is a meaningful outcome, not an error: it means the target
/// is composed in-process and must be consumed through its declared in-process
/// port.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedBaseUrl {
    service: String,
    base_url: Option<String>,
    source: BaseUrlSource,
}

impl ResolvedBaseUrl {
    /// The resolved base URL, or `None` when the target is in-process.
    pub fn base_url(&self) -> Option<&str> {
        self.base_url.as_deref()
    }

    /// Where the value came from.
    pub fn source(&self) -> BaseUrlSource {
        self.source
    }

    /// The service identifier this resolution was performed for.
    pub fn service(&self) -> &str {
        &self.service
    }

    /// Whether the target is composed into this process (no HTTP hop).
    pub fn is_embedded(&self) -> bool {
        self.source == BaseUrlSource::EmbeddedInProcess
    }

    /// Consumes the resolution, yielding the base URL or a diagnostic.
    ///
    /// Split-mode callers that cannot represent "no transport" use this to turn
    /// the in-process outcome into one actionable message instead of silently
    /// dialling a loopback.
    pub fn into_required(self) -> Result<String, BaseUrlUnavailable> {
        match self.base_url {
            Some(base_url) => Ok(base_url),
            None => Err(BaseUrlUnavailable {
                service: self.service,
            }),
        }
    }
}

/// Returned when a caller requires an HTTP base URL but the target is composed
/// in-process.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BaseUrlUnavailable {
    service: String,
}

impl BaseUrlUnavailable {
    /// The service that has no HTTP transport in this topology.
    pub fn service(&self) -> &str {
        &self.service
    }
}

impl std::fmt::Display for BaseUrlUnavailable {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            formatter,
            "service `{}` has no HTTP base URL: it is composed into this process, so it must be \
             consumed through its declared in-process port (APPLICATION_GATEWAY_SPEC §2.3, \
             APP_SDK_INTEGRATION_SPEC §5.2). Set the explicit override only for a split deployment.",
            self.service
        )
    }
}

impl std::error::Error for BaseUrlUnavailable {}

/// One service's base-URL resolution inputs.
///
/// The inputs are supplied by the caller (already read from the environment or
/// topology) rather than read here, so the decision is a pure function and unit
/// tests need no process-global environment mutation.
#[derive(Debug, Clone)]
pub struct ServiceBaseUrlRequest {
    service: String,
    mode: ServiceMode,
    explicit_override: Option<String>,
    authored_public_url: Option<String>,
    split_default: Option<String>,
}

impl ServiceBaseUrlRequest {
    /// Builds a request for `service` under the given deployment `mode`.
    pub fn new(service: impl Into<String>, mode: ServiceMode) -> Self {
        Self {
            service: service.into(),
            mode,
            explicit_override: None,
            authored_public_url: None,
            split_default: None,
        }
    }

    /// Sets the explicit per-service override (highest precedence).
    pub fn with_explicit_override(mut self, value: Option<String>) -> Self {
        self.explicit_override = normalize(value);
        self
    }

    /// Sets the topology's authored public HTTP URL for the surface.
    pub fn with_authored_public_url(mut self, value: Option<String>) -> Self {
        self.authored_public_url = normalize(value);
        self
    }

    /// Sets the compiled-in default used when no authored value is present.
    pub fn with_split_default(mut self, value: impl Into<String>) -> Self {
        self.split_default = normalize(Some(value.into()));
        self
    }

    /// The deployment mode this request resolves under.
    pub fn mode(&self) -> ServiceMode {
        self.mode
    }
}

/// Normalizes an optional environment value: trims and drops empties, so a
/// declared-but-blank variable behaves like an absent one.
fn normalize(value: Option<String>) -> Option<String> {
    value
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

/// Resolves the base URL for one service.
///
/// Precedence:
/// 1. the explicit override, in either mode — the sanctioned escape hatch for a
///    split deployment that hosts an otherwise in-process surface separately;
/// 2. in [`ServiceMode::Embedded`], stop: the target is composed in-process, so
///    there is no HTTP base URL and none is derived;
/// 3. a healthy instance selected from the process' [`HttpServiceRegistry`]
///    (the database-backed discovery registry; see `DISCOVERY_SPEC.md`);
/// 4. the authored public HTTP URL (an absolute domain in cloud);
/// 5. the split default — the documented degradation path when neither the
///    registry nor the topology yields a value.
pub fn resolve(request: &ServiceBaseUrlRequest) -> ResolvedBaseUrl {
    resolve_with_registry(
        request,
        installed_http_service_registry().map(|registry| registry.as_ref()),
    )
}

/// Pure form of [`resolve`] with an explicit registry view.
///
/// Keeping the decision injectable lets unit tests exercise registry precedence
/// without installing a process-global registry (which cannot be uninstalled).
pub fn resolve_with_registry(
    request: &ServiceBaseUrlRequest,
    registry: Option<&dyn HttpServiceRegistry>,
) -> ResolvedBaseUrl {
    let service = request.service.clone();

    if let Some(value) = request.explicit_override.clone() {
        return ResolvedBaseUrl {
            service,
            base_url: Some(value),
            source: BaseUrlSource::ExplicitOverride,
        };
    }

    if request.mode == ServiceMode::Embedded {
        // Explicitly stopping before the authored URL matters: an embedded
        // profile's authored URL is typically a loopback bind, and adopting it
        // here is exactly the self-loop this component exists to prevent.
        return ResolvedBaseUrl {
            service,
            base_url: None,
            source: BaseUrlSource::EmbeddedInProcess,
        };
    }

    if let Some(registry) = registry {
        if let Some(target) = registry.resolve_http(&service) {
            return ResolvedBaseUrl {
                service,
                base_url: Some(target.base_url),
                source: BaseUrlSource::Registry,
            };
        }
    }

    if let Some(value) = request.authored_public_url.clone() {
        return ResolvedBaseUrl {
            service,
            base_url: Some(value),
            source: BaseUrlSource::AuthoredPublicUrl,
        };
    }

    ResolvedBaseUrl {
        service,
        base_url: request.split_default.clone(),
        source: BaseUrlSource::SplitDefault,
    }
}

/// Classifies a deployment profile string into a [`ServiceMode`].
///
/// Only the canonical `standalone` profile means "embedded". Every other
/// profile (`cloud`, `test`, a custom name) is a split deployment, because a
/// surface is composed in-process only when the composition root says so — and
/// a composition root always runs the standalone profile.
pub fn mode_of_deployment_profile(profile: Option<&str>) -> ServiceMode {
    match profile.map(str::trim) {
        Some(profile) if profile.eq_ignore_ascii_case("standalone") => ServiceMode::Embedded,
        _ => ServiceMode::Split,
    }
}

/// Whether a deployment profile names the embedded (`standalone`) topology.
///
/// Exposed separately from [`mode_of_deployment_profile`] because consumers
/// repeatedly need the bare predicate — gating packaged-runtime roots, an
/// adaptive shell, or a static-delivery path — and each hand-rolled copy
/// normalizes differently (`==` here, `trim().eq_ignore_ascii_case()` there),
/// which lets two parts of one process disagree about its own mode. A missing
/// or blank profile is treated as `standalone`, matching the runtime default.
pub fn is_standalone_profile(profile: Option<&str>) -> bool {
    match profile.map(str::trim) {
        None => true,
        Some(profile) if profile.is_empty() => true,
        Some(profile) => profile.eq_ignore_ascii_case("standalone"),
    }
}

/// Extracts the TCP port from an `address:port` bind string.
///
/// Accepts the forms the topology profiles actually declare: `0.0.0.0:3905`,
/// `127.0.0.1:3900`, `[::]:3907`, `host:3906`. Returns `None` for anything
/// without a numeric trailing port, so a malformed bind is reported rather than
/// silently coerced.
///
/// This is a *listener* helper: the returned port is for binding or for a
/// declared in-process port, never for composing an outbound loopback URL.
pub fn bind_port(bind: &str) -> Option<u16> {
    let trimmed = bind.trim();
    let (_, port) = trimmed.rsplit_once(':')?;
    if port.is_empty() || !port.chars().all(|character| character.is_ascii_digit()) {
        return None;
    }
    port.parse::<u16>().ok()
}

/// Whether an env value looks like a loopback origin.
///
/// Used by callers that must refuse a loopback base URL for a surface they
/// already compose in-process — the failure mode is otherwise invisible until
/// the request times out against a port nobody owns.
pub fn is_loopback_origin(value: &str) -> bool {
    let Some((_, rest)) = value.trim().split_once("://") else {
        return false;
    };
    let authority = rest.split(['/', '?', '#']).next().unwrap_or("");
    let host = match authority.strip_prefix('[') {
        // IPv6 literal: `[::1]:3905`
        Some(rest) => rest.split(']').next().unwrap_or("").to_ascii_lowercase(),
        None => authority
            .rsplit_once(':')
            .map(|(host, _)| host)
            .unwrap_or(authority)
            .to_ascii_lowercase(),
    };
    matches!(
        host.as_str(),
        "127.0.0.1" | "localhost" | "0.0.0.0" | "::1" | "::" | ""
    )
}

/// Picks the first non-blank candidate as a listener bind address.
///
/// A *listener* helper, the counterpart to [`bind_port`]: its result feeds
/// `TcpListener::bind`, never an outbound client. Every gateway resolves its
/// bind through this so the precedence (application bind, then server bind,
/// then a runtime-config value, then the compiled default) is defined once
/// instead of being re-implemented per binary.
///
/// Candidates are supplied by the caller, so the ordering is explicit at the
/// call site and unit-testable without touching the process environment.
pub fn resolve_listener_bind<'a, I>(candidates: I, fallback: &'a str) -> String
where
    I: IntoIterator<Item = Option<&'a str>>,
{
    candidates
        .into_iter()
        .flatten()
        .map(str::trim)
        .find(|value| !value.is_empty())
        .map(str::to_owned)
        .unwrap_or_else(|| fallback.to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn split(service: &str) -> ServiceBaseUrlRequest {
        ServiceBaseUrlRequest::new(service, ServiceMode::Split)
    }

    fn embedded(service: &str) -> ServiceBaseUrlRequest {
        ServiceBaseUrlRequest::new(service, ServiceMode::Embedded)
    }

    /// A registry view with one healthy target and no failure tracking.
    struct StaticRegistry {
        target: RegistryHttpTarget,
    }

    impl HttpServiceRegistry for StaticRegistry {
        fn resolve_http(&self, service: &str) -> Option<RegistryHttpTarget> {
            (service == self.target.service).then(|| self.target.clone())
        }

        fn report_failure(&self, _target: &RegistryHttpTarget, _reason: &str) {}

        fn report_success(&self, _target: &RegistryHttpTarget) {}
    }

    fn static_registry(service: &str, base_url: &str) -> StaticRegistry {
        StaticRegistry {
            target: RegistryHttpTarget {
                service: service.to_owned(),
                instance_id: "instance-1".to_owned(),
                base_url: base_url.to_owned(),
            },
        }
    }

    #[test]
    fn registry_base_url_falls_back_to_the_configured_value() {
        // No process registry is installed in this test binary, so the helper
        // must return the consumer's configured value unchanged.
        assert_eq!(
            registry_base_url_or("never-registered", "https://configured.example.com/"),
            "https://configured.example.com"
        );
    }

    #[test]
    fn registry_healthy_instance_wins_over_the_authored_and_default_values() {
        let registry = static_registry("cloudrouter", "https://registry.example.com");
        let resolved = resolve_with_registry(
            &split("cloudrouter")
                .with_authored_public_url(Some("https://authored.example.com".into()))
                .with_split_default("http://127.0.0.1:3900"),
            Some(&registry),
        );
        assert_eq!(resolved.base_url(), Some("https://registry.example.com"));
        assert_eq!(resolved.source(), BaseUrlSource::Registry);
    }

    #[test]
    fn explicit_override_wins_over_the_registry() {
        let registry = static_registry("cloudrouter", "https://registry.example.com");
        let resolved = resolve_with_registry(
            &split("cloudrouter")
                .with_explicit_override(Some("https://override.example.com".into())),
            Some(&registry),
        );
        assert_eq!(resolved.base_url(), Some("https://override.example.com"));
        assert_eq!(resolved.source(), BaseUrlSource::ExplicitOverride);
    }

    #[test]
    fn embedded_mode_never_consults_the_registry() {
        let registry = static_registry("cloudrouter", "https://registry.example.com");
        let resolved = resolve_with_registry(&embedded("cloudrouter"), Some(&registry));
        assert_eq!(resolved.base_url(), None);
        assert_eq!(resolved.source(), BaseUrlSource::EmbeddedInProcess);
    }

    #[test]
    fn an_empty_registry_degrades_to_the_authored_value_then_the_default() {
        let registry = static_registry("other-service", "https://registry.example.com");
        let authored = resolve_with_registry(
            &split("cloudrouter")
                .with_authored_public_url(Some("https://authored.example.com".into()))
                .with_split_default("http://127.0.0.1:3900"),
            Some(&registry),
        );
        assert_eq!(authored.base_url(), Some("https://authored.example.com"));
        assert_eq!(authored.source(), BaseUrlSource::AuthoredPublicUrl);

        let fallback = resolve_with_registry(
            &split("cloudrouter").with_split_default("http://127.0.0.1:3900"),
            Some(&registry),
        );
        assert_eq!(fallback.base_url(), Some("http://127.0.0.1:3900"));
        assert_eq!(fallback.source(), BaseUrlSource::SplitDefault);
    }

    #[test]
    fn split_mode_prefers_the_explicit_override() {
        let resolved = resolve(
            &split("cloudrouter")
                .with_explicit_override(Some("https://router.sdkwork.com".into()))
                .with_authored_public_url(Some("https://authored.example.com".into()))
                .with_split_default("http://127.0.0.1:3900"),
        );
        assert_eq!(resolved.base_url(), Some("https://router.sdkwork.com"));
        assert_eq!(resolved.source(), BaseUrlSource::ExplicitOverride);
        assert!(!resolved.is_embedded());
    }

    #[test]
    fn split_mode_uses_the_authored_public_url_over_the_default() {
        // Cloud publishes an absolute domain; that must win over the compiled
        // default so a production deployment never dials a loopback.
        let resolved = resolve(
            &split("cloudrouter")
                .with_authored_public_url(Some("https://router-staging.sdkwork.com".into()))
                .with_split_default("http://127.0.0.1:3900"),
        );
        assert_eq!(
            resolved.base_url(),
            Some("https://router-staging.sdkwork.com")
        );
        assert_eq!(resolved.source(), BaseUrlSource::AuthoredPublicUrl);
    }

    #[test]
    fn split_mode_falls_back_to_the_compiled_default() {
        let resolved = resolve(&split("cloudrouter").with_split_default("http://127.0.0.1:3900"));
        assert_eq!(resolved.base_url(), Some("http://127.0.0.1:3900"));
        assert_eq!(resolved.source(), BaseUrlSource::SplitDefault);
    }

    #[test]
    fn embedded_mode_yields_no_base_url_even_with_an_authored_loopback() {
        // The regression that caused the production `Bad gateway` incident: a
        // standalone profile authors its public URL as `http://127.0.0.1:3905`,
        // and adopting it while the surface is composed in-process is a
        // self-loop. Embedded mode must stop before consulting it.
        let resolved = resolve(
            &embedded("cloudrouter")
                .with_authored_public_url(Some("http://127.0.0.1:3905".into()))
                .with_split_default("http://127.0.0.1:3900"),
        );
        assert_eq!(resolved.base_url(), None);
        assert_eq!(resolved.source(), BaseUrlSource::EmbeddedInProcess);
        assert!(resolved.is_embedded());
    }

    #[test]
    fn embedded_mode_still_honours_an_explicit_override() {
        // A split deployment that hosts an otherwise-embedded surface
        // separately sets the override; it is the sanctioned escape hatch.
        let resolved = resolve(
            &embedded("cloudrouter")
                .with_explicit_override(Some("https://router.sdkwork.com".into()))
                .with_authored_public_url(Some("http://127.0.0.1:3905".into())),
        );
        assert_eq!(resolved.base_url(), Some("https://router.sdkwork.com"));
        assert_eq!(resolved.source(), BaseUrlSource::ExplicitOverride);
    }

    #[test]
    fn blank_values_behave_like_absent_ones() {
        let resolved = resolve(
            &split("cloudrouter")
                .with_explicit_override(Some("   ".into()))
                .with_authored_public_url(Some(String::new()))
                .with_split_default("http://127.0.0.1:3900"),
        );
        assert_eq!(resolved.base_url(), Some("http://127.0.0.1:3900"));
        assert_eq!(resolved.source(), BaseUrlSource::SplitDefault);
    }

    #[test]
    fn values_are_trimmed_before_use() {
        let resolved = resolve(
            &split("cloudrouter")
                .with_explicit_override(Some("  https://router.sdkwork.com  ".into())),
        );
        assert_eq!(resolved.base_url(), Some("https://router.sdkwork.com"));
    }

    #[test]
    fn into_required_reports_the_in_process_outcome_actionably() {
        let resolved = resolve(&embedded("cloudrouter"));
        let error = resolved
            .into_required()
            .expect_err("no base URL in-process");
        assert_eq!(error.service(), "cloudrouter");
        let message = error.to_string();
        assert!(message.contains("cloudrouter"));
        assert!(message.contains("in-process"));
        // The message must name the governing specs so the fix is discoverable.
        assert!(message.contains("APPLICATION_GATEWAY_SPEC"));
        assert!(message.contains("APP_SDK_INTEGRATION_SPEC"));
    }

    #[test]
    fn into_required_passes_through_a_resolved_url() {
        let resolved = resolve(
            &split("cloudrouter").with_explicit_override(Some("https://router.sdkwork.com".into())),
        );
        assert_eq!(
            resolved.into_required().expect("resolved"),
            "https://router.sdkwork.com"
        );
    }

    #[test]
    fn only_the_standalone_profile_is_embedded() {
        assert_eq!(
            mode_of_deployment_profile(Some("standalone")),
            ServiceMode::Embedded
        );
        assert_eq!(
            mode_of_deployment_profile(Some("STANDALONE")),
            ServiceMode::Embedded
        );
        assert_eq!(
            mode_of_deployment_profile(Some("  standalone  ")),
            ServiceMode::Embedded
        );
        assert_eq!(
            mode_of_deployment_profile(Some("cloud")),
            ServiceMode::Split
        );
        assert_eq!(mode_of_deployment_profile(Some("test")), ServiceMode::Split);
        // An absent or blank profile is a split deployment: a surface is
        // composed in-process only when the composition root says so.
        assert_eq!(mode_of_deployment_profile(None), ServiceMode::Split);
        assert_eq!(mode_of_deployment_profile(Some("")), ServiceMode::Split);
        assert_eq!(mode_of_deployment_profile(Some("   ")), ServiceMode::Split);
    }

    #[test]
    fn is_standalone_profile_matches_the_runtime_default() {
        assert!(is_standalone_profile(Some("standalone")));
        assert!(is_standalone_profile(Some("STANDALONE")));
        assert!(is_standalone_profile(Some("  Standalone  ")));
        // Absent or blank resolves to the supplied runtime default, which is
        // `standalone` in every SDKWork composition root. Consumers that gate
        // on `cloud` must therefore not treat a blank value as cloud.
        assert!(is_standalone_profile(None));
        assert!(is_standalone_profile(Some("")));
        assert!(is_standalone_profile(Some("   ")));
        assert!(!is_standalone_profile(Some("cloud")));
        assert!(!is_standalone_profile(Some("CLOUD")));
        assert!(!is_standalone_profile(Some("test")));
        // Unrecognised values are not standalone: the gate errs toward the
        // split path, where an explicit topology value is required.
        assert!(!is_standalone_profile(Some("standalon")));
    }

    #[test]
    fn bind_port_accepts_every_topology_form() {
        assert_eq!(bind_port("0.0.0.0:3905"), Some(3905));
        assert_eq!(bind_port("127.0.0.1:3900"), Some(3900));
        assert_eq!(bind_port("[::]:3907"), Some(3907));
        assert_eq!(bind_port("gateway:3906"), Some(3906));
        assert_eq!(bind_port("  0.0.0.0:8080  "), Some(8080));
        // Malformed binds are reported, never silently coerced.
        assert_eq!(bind_port("no-port"), None);
        assert_eq!(bind_port("host:"), None);
        assert_eq!(bind_port("host:39x0"), None);
        assert_eq!(bind_port("host:99999"), None);
        assert_eq!(bind_port(""), None);
    }

    #[test]
    fn is_loopback_origin_recognizes_the_prohibited_shapes() {
        assert!(is_loopback_origin("http://127.0.0.1:3905"));
        assert!(is_loopback_origin("http://localhost:3900"));
        assert!(is_loopback_origin("http://0.0.0.0:3905"));
        assert!(is_loopback_origin("https://[::1]:3905"));
        assert!(is_loopback_origin("http://127.0.0.1:3905/backend/v3/api"));
        assert!(is_loopback_origin("HTTP://127.0.0.1:3905"));

        assert!(!is_loopback_origin("https://router.sdkwork.com"));
        assert!(!is_loopback_origin("https://api.sdkwork.com/backend"));
        assert!(!is_loopback_origin("/backend/v3/api"));
        assert!(!is_loopback_origin("127.0.0.1:3905"));
    }

    #[test]
    fn source_labels_are_stable_for_logs_and_problem_details() {
        assert_eq!(
            BaseUrlSource::ExplicitOverride.as_str(),
            "explicit_override"
        );
        assert_eq!(
            BaseUrlSource::AuthoredPublicUrl.as_str(),
            "authored_public_url"
        );
        assert_eq!(BaseUrlSource::SplitDefault.as_str(), "split_default");
        assert_eq!(
            BaseUrlSource::EmbeddedInProcess.as_str(),
            "embedded_in_process"
        );
    }

    #[test]
    fn resolution_reports_the_service_it_was_asked_about() {
        let resolved = resolve(&embedded("sdkwork-agents"));
        assert_eq!(resolved.service(), "sdkwork-agents");
    }

    #[test]
    fn resolve_listener_bind_takes_the_first_non_blank_candidate() {
        // The gateway precedence: application bind, then server bind, then the
        // runtime-config value, then the compiled default.
        assert_eq!(
            resolve_listener_bind(
                [Some("0.0.0.0:3905"), Some("127.0.0.1:9999"), None],
                "127.0.0.1:3905"
            ),
            "0.0.0.0:3905"
        );
        // A blank or absent leading candidate is skipped, not adopted.
        assert_eq!(
            resolve_listener_bind([Some("  "), None, Some("0.0.0.0:8080")], "127.0.0.1:3905"),
            "0.0.0.0:8080"
        );
        assert_eq!(
            resolve_listener_bind([None, Some("   ")], "127.0.0.1:3905"),
            "127.0.0.1:3905"
        );
        assert_eq!(
            resolve_listener_bind(Vec::<Option<&str>>::new(), "127.0.0.1:3905"),
            "127.0.0.1:3905"
        );
    }

    #[test]
    fn resolve_listener_bind_trims_the_selected_value() {
        assert_eq!(
            resolve_listener_bind([Some("  0.0.0.0:3905  ")], "fallback:1"),
            "0.0.0.0:3905"
        );
    }
}
