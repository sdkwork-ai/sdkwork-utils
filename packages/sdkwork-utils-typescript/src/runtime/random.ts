function getCrypto(): Crypto {
  const crypto = globalThis.crypto;
  if (!crypto?.getRandomValues) {
    throw new Error("Web Crypto API is not available in this environment.");
  }
  return crypto;
}

export function randomBytes(length: number): Uint8Array {
  const bytes = new Uint8Array(length);
  getCrypto().getRandomValues(bytes);
  return bytes;
}

/**
 * RFC 4122 version-4 UUID.
 *
 * Prefer `crypto.randomUUID` when it is a real function (secure contexts).
 * Fall back to `getRandomValues` for HTTP / older hosts / broken shims so
 * browser admin consoles never crash on create/idempotency flows.
 */
export function randomUuid(): string {
  const crypto = getCrypto();
  if (typeof crypto.randomUUID === "function") {
    try {
      return crypto.randomUUID.call(crypto);
    } catch {
      // Non-secure contexts and some polyfills expose a non-callable or
      // throwing randomUUID; continue with getRandomValues.
    }
  }

  const bytes = randomBytes(16);
  bytes[6] = ((bytes[6] ?? 0) & 0x0f) | 0x40;
  bytes[8] = ((bytes[8] ?? 0) & 0x3f) | 0x80;

  const hex = Array.from(bytes, (byte) => byte.toString(16).padStart(2, "0")).join("");
  return `${hex.slice(0, 8)}-${hex.slice(8, 12)}-${hex.slice(12, 16)}-${hex.slice(16, 20)}-${hex.slice(20)}`;
}
