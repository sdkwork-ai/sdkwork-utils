export declare function randomBytes(length: number): Uint8Array;
/**
 * RFC 4122 version-4 UUID.
 *
 * Prefer `crypto.randomUUID` when it is a real function (secure contexts).
 * Fall back to `getRandomValues` for HTTP / older hosts / broken shims so
 * browser admin consoles never crash on create/idempotency flows.
 */
export declare function randomUuid(): string;
//# sourceMappingURL=random.d.ts.map