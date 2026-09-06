# @sdkwork/utils

Domain: shared utilities
Capability: cross-language common helpers
Package type: library
Status: standard
Version: 0.11.0

TypeScript / Node implementation of the SDKWork utils contract.

The npm package name is `@sdkwork/utils`. The repository directory remains
`packages/sdkwork-utils-typescript` because this package is one language binding
inside the cross-language `sdkwork-utils` workspace.

## Public API

```typescript
import {
  isBlank,
  camelCase,
  parseDatetime,
  urlEncode,
  parseBool,
  ResultValue,
} from "@sdkwork/utils";

isBlank("  "); // true
camelCase("hello_world"); // "helloWorld"
urlEncode("hello world"); // "hello%20world"
```

Subpath imports are available for tree-shaking:

```typescript
import { isBlank, camelCase } from "@sdkwork/utils/string";
import { defaultIfBlank } from "@sdkwork/utils/optional";
```

Exports use camelCase. See [`../../specs/naming.aliases.json`](../../specs/naming.aliases.json).

## Configuration

```bash
pnpm add @sdkwork/utils
# or path dependency during monorepo development
```

Build before use:

```bash
pnpm --dir packages/sdkwork-utils-typescript build
```

## Verification

```bash
pnpm --dir packages/sdkwork-utils-typescript test
```

## Related specs

- [`../../specs/utils.contract.json`](../../specs/utils.contract.json)
- [`../../../sdkwork-specs/NAMING_SPEC.md`](../../../sdkwork-specs/NAMING_SPEC.md)
- [`../../../sdkwork-specs/SDK_SPEC.md`](../../../sdkwork-specs/SDK_SPEC.md)

## Migration

`@sdkwork/utils-typescript` is retired. Replace imports with `@sdkwork/utils`
and matching subpaths such as `@sdkwork/utils/string`.
