# sdkwork-utils-rust

Domain: shared utilities  
Capability: cross-language common helpers  
Package type: library  
Status: standard  
Version: 0.11.0

Rust implementation of the SDKWork utils contract.

## Public API

Crate: `sdkwork-utils-rust`  
Modules mirror the contract: `string`, `datetime`, `number`, `currency`, `bloom`, `bytes`, `collection`, `validation`, `id`, `encoding`, `path`, `object`, `crypto`, `optional`, `result`, `i18n`, `boolean`, `compare`.

Platform services additionally expose:

- `trusted_proxy`: forwarded-chain parsing against a trusted-proxy CIDR allow-list.
- `serde_int64`: i64 wire serialization for the `API_SPEC` §13.6 int64-as-string contract.
- `service_base_url`: the single backend base-URL and listener-bind resolver (see below).

All items are re-exported from the crate root:

```rust
use sdkwork_utils_rust::{is_blank, camel_case, parse_datetime, ResultValue};

assert!(is_blank(None));
assert_eq!(camel_case("hello_world"), "helloWorld");
```

Contract operations use snake_case (see [`../../specs/naming.aliases.json`](../../specs/naming.aliases.json)).

## `service_base_url` — base-URL and bind resolution

Backend SDK clients must not each invent their own base-URL precedence. This module owns that
decision once, so `standalone` (embedded, same-origin, in-process) and `cloud` (split, authored
absolute domain) deployments cannot drift apart or accidentally self-loop.

```rust
use sdkwork_utils_rust::service_base_url::{
    mode_of_deployment_profile, resolve, ServiceBaseUrlRequest,
};

let request = ServiceBaseUrlRequest::new(
    "cloudrouter",
    mode_of_deployment_profile(std::env::var("SDKWORK_DEPLOYMENT_PROFILE").ok().as_deref()),
)
.with_explicit_override(std::env::var("SDKWORK_CLOUDROUTER_BASE_URL").ok())
.with_authored_public_url(std::env::var("SDKWORK_CLOUDROUTER_APPLICATION_PUBLIC_HTTP_URL").ok())
.with_split_default("http://127.0.0.1:3900");

let resolved = resolve(&request);
assert_eq!(resolved.source().as_str(), "authored_public_url");
```

Resolution order:

1. `explicit_override` wins in **both** modes — a deliberate per-dependency override is honoured
   even when the dependency is embedded in-process.
2. `ServiceMode::Embedded` then resolves to **no base URL** (`None`). An embedded dependency is
   consumed through a declared in-process port; there is no HTTP hop to address. The authored public
   URL is deliberately *not* consulted here, because under a standalone profile it is itself a
   loopback value and adopting it is exactly the self-loop this module prevents
   (`APPLICATION_GATEWAY_SPEC` §2.3, `API_ASSEMBLY_SPEC` §6.1).
3. `ServiceMode::Split` falls back to `authored_public_url`, then to `split_default`.

Supporting helpers live in the same module so the whole surface has one implementation:
`bind_port`, `is_loopback_origin`, and `resolve_listener_bind` (first non-blank candidate, then
fallback).

The resolver is a **pure function** of its request; it never reads the process environment. Callers
read the environment and pass the values in, which keeps every branch unit-testable and free of
process-global state races.

`ResolvedBaseUrl` carries its own provenance: `service()`, `source()` (`BaseUrlSource`), and
`is_embedded()`. Consumers are expected to log that provenance and surface it in problem details, so
an operator can tell "no base URL because embedded" apart from "base URL missing". `into_required()`
converts the embedded outcome into an actionable `BaseUrlUnavailable` error whose message names the
service and the two specs above.

See `APP_SDK_INTEGRATION_SPEC.md` §5.2 and §7 in `sdkwork-specs` for the consumer-side contract.

## Configuration

No runtime configuration. Add to your workspace `Cargo.toml`:

```toml
[dependencies]
sdkwork-utils-rust = { path = "../sdkwork-utils/packages/sdkwork-utils-rust" }
```

## Verification

From repository root:

```bash
cargo test --workspace
```

Or:

```bash
pnpm verify
```

Conformance tests: `tests/conformance.rs` (reads shared fixtures).

## Related specs

- [`../../specs/utils.contract.json`](../../specs/utils.contract.json)
- [`../../AGENTS.md`](../../AGENTS.md)
