# sdkwork-utils specs

Authoritative contracts for the cross-language utility library.

| File | Purpose |
| --- | --- |
| [`utils.contract.json`](utils.contract.json) | Module and operation definitions (v0.11, 117 operations) |
| [`naming.aliases.json`](naming.aliases.json) | Idiomatic export names per language |
| [`conformance/fixtures.json`](conformance/fixtures.json) | Shared behavioral test vectors |

Beyond the cross-language contract, the Rust package also owns platform-service helpers that are
deliberately single-language because they touch process/deployment concerns:

| Module | Purpose | Authority |
| --- | --- | --- |
| `service_base_url` | The one backend base-URL and listener-bind resolver for embedded vs split deployments | `sdkwork-specs/APP_SDK_INTEGRATION_SPEC.md` §5.2 / §7 |
| `trusted_proxy` | Forwarded-chain parsing against a trusted-proxy CIDR allow-list | `sdkwork-specs/SECURITY_SPEC.md` |
| `serde_int64` | i64 wire serialization for int64-as-string | `sdkwork-specs/API_SPEC.md` §13.6 |

These follow the same change discipline as the contract modules: update the module, its unit tests,
the consumer-side authority spec, and the affected call sites in one change.

When changing public behavior:

1. Update `utils.contract.json` and bump version if breaking.
2. Add or update fixtures in `conformance/fixtures.json`.
3. Implement in all eight language packages under `packages/`.
4. Run `pnpm verify` from repository root.

See [`../AGENTS.md`](../AGENTS.md) for agent execution rules.
