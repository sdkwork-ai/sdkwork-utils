/**
 * Token Bank points precision helpers.
 *
 * Token Bank points carry up to 6 decimal places so fractional usage debits
 * and account balances stay exact. Internally the ledger and database store
 * those points as integer **micro-points** (1 point = 1_000_000 micro points)
 * so a single integer numeric unit is shared everywhere and no floating point
 * drift is ever introduced.
 *
 * These helpers convert between the integer micro representation (bigint) and
 * a 6-decimal string used at parse/display boundaries.
 */

/** Number of decimal digits a Token Bank point carries. */
export const TOKEN_POINTS_SCALE = 6;

/** Micro-points that make up a single whole point. */
export const MICRO_POINTS_PER_POINT = 1_000_000n;

/**
 * Convert a non-negative micro-point bigint into a points decimal string with
 * up to 6 fractional digits. Trailing fractional zeros are trimmed.
 */
export function microToDecimalString(micro: bigint): string {
  if (micro < 0n) return "0";
  const whole = micro / MICRO_POINTS_PER_POINT;
  const fraction = micro % MICRO_POINTS_PER_POINT;
  if (fraction === 0n) return whole.toString();
  const scaled = fraction.toString().padStart(TOKEN_POINTS_SCALE, "0");
  return `${whole.toString()}.${scaled.replace(/0+$/, "")}`;
}

function parseDecimal(value: string): { whole: bigint; fraction: string } | null {
  const trimmed = value.trim();
  if (trimmed.length === 0 || trimmed.startsWith("-") || trimmed.startsWith("+")) {
    return null;
  }
  const dot = trimmed.indexOf(".");
  if (dot !== -1 && trimmed.indexOf(".", dot + 1) !== -1) {
    return null;
  }
  const whole = dot === -1 ? trimmed : trimmed.slice(0, dot);
  const fraction = dot === -1 ? "" : trimmed.slice(dot + 1);
  if (whole.length === 0) return null;
  if (!/^\d+$/.test(whole)) return null;
  if (fraction.length > 0 && (!/^\d+$/.test(fraction) || fraction.length > TOKEN_POINTS_SCALE)) {
    return null;
  }
  if (dot !== -1 && fraction.length === 0) return null;
  let wholeValue: bigint;
  try {
    wholeValue = BigInt(whole);
  } catch {
    return null;
  }
  return { whole: wholeValue, fraction };
}

/**
 * Parse a points decimal string into an integer micro-point bigint.
 * The value must be a non-negative decimal with at most 6 fractional digits.
 * Returns `null` for negatives, malformed input, or values that do not fit
 * a 64-bit signed integer.
 */
export function decimalStringToMicro(value: string): bigint | null {
  const parsed = parseDecimal(value);
  if (!parsed) return null;
  const pad = TOKEN_POINTS_SCALE - parsed.fraction.length;
  let micro = parsed.whole * MICRO_POINTS_PER_POINT;
  if (parsed.fraction.length > 0) {
    micro += BigInt(parsed.fraction + "0".repeat(pad));
  }
  if (micro > 9_223_372_036_854_775_807n) return null; // i64 max
  return micro;
}