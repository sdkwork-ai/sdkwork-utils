/**
 * Exact decimal arithmetic on integer units.
 *
 * All money / points / token-bank / exchange-rate math that must never lose a
 * fraction is funneled through these helpers. A decimal string is parsed into
 * an integer count of `scale`-th units (e.g. scale 6 => `1.5` -> `1500000`,
 * scale 12 => `0.000704` -> `704000000`), the operation is performed in exact
 * BigInt integer arithmetic, a rounding mode is applied only when a scaling
 * boundary is crossed, and the result is formatted back to a plain decimal
 * string. No `number` (IEEE-754 double) is ever used, so results are exact for
 * arbitrary input magnitude and fractional digits.
 *
 * Rounding modes mirror standard decimal conventions and are only applied at
 * the requested `scale` boundary:
 *  - `"floor"`   truncate toward -infinity
 *  - `"ceil"`    round toward +infinity (billing uses this so a fractional
 *                charge never shortchanges the merchant)
 *  - `"half_up"` round half away from zero
 *  - `"half_even"` banker's rounding (round half to even)
 */

export type DecimalRounding = "floor" | "ceil" | "half_up" | "half_even";

/** The default fractional scale shared by the arithmetic helpers. */
export const DEFAULT_DECIMAL_SCALE = 12;

function normalizeScale(scale: number): number {
  if (!Number.isSafeInteger(scale) || scale < 0 || scale > 38) {
    return DEFAULT_DECIMAL_SCALE;
  }
  return scale;
}

function assertRounding(rounding: DecimalRounding): DecimalRounding {
  if (rounding === "floor" || rounding === "ceil" || rounding === "half_up" || rounding === "half_even") {
    return rounding;
  }
  return "floor";
}

const DECIMAL_PATTERN = /^-?\d+(?:\.\d+)?$/u;

/**
 * Parse a signed decimal string into the integer count of `scale`-th units.
 * A value carrying more fractional digits than `scale` is rounded to `scale`
 * with `rounding`. Returns `null` for empty / malformed input.
 */
export function decimalToScaledUnits(
  value: string,
  scale = DEFAULT_DECIMAL_SCALE,
  rounding: DecimalRounding = "half_up",
): bigint | null {
  const normalizedScale = normalizeScale(scale);
  const mode = assertRounding(rounding);
  const trimmed = value.trim();
  if (!DECIMAL_PATTERN.test(trimmed)) {
    return null;
  }
  const negative = trimmed.startsWith("-");
  const unsigned = negative ? trimmed.slice(1) : trimmed;
  const [wholeRaw = "0", fractionRaw = ""] = unsigned.split(".");
  let whole: bigint;
  try {
    whole = BigInt(wholeRaw);
  } catch {
    return null;
  }
  const n = fractionRaw.length;
  let raw: bigint;
  try {
    raw = whole * 10n ** BigInt(n) + (n > 0 ? BigInt(fractionRaw) : 0n);
  } catch {
    return null;
  }
  let units: bigint;
  if (n <= normalizedScale) {
    units = raw * 10n ** BigInt(normalizedScale - n);
  } else {
    const denominator = 10n ** BigInt(n - normalizedScale);
    units = roundFraction(raw, denominator, mode);
  }
  return negative ? -units : units;
}

/**
 * Parse the exact value of a signed decimal string into integer units without
 * any loss: returns the integer digits and the number of fractional digits.
 * Used internally by arithmetic so the product / quotient stays exact until a
 * final rounding at the requested scale.
 */
function parseExact(value: string): { digits: bigint; scale: number } | null {
  const trimmed = value.trim();
  if (!DECIMAL_PATTERN.test(trimmed)) {
    return null;
  }
  const negative = trimmed.startsWith("-");
  const unsigned = negative ? trimmed.slice(1) : trimmed;
  const [wholeRaw = "0", fractionRaw = ""] = unsigned.split(".");
  let whole: bigint;
  try {
    whole = BigInt(wholeRaw);
  } catch {
    return null;
  }
  const trimmedFraction = fractionRaw.replace(/0+$/u, "");
  let digits = whole * 10n ** BigInt(trimmedFraction.length);
  if (trimmedFraction.length > 0) {
    digits += BigInt(trimmedFraction);
  }
  if (negative) {
    digits = -digits;
  }
  return { digits, scale: trimmedFraction.length };
}

/** Round `num / denom` to an integer using `rounding` (denom must be positive). */
function roundFraction(num: bigint, denom: bigint, rounding: DecimalRounding): bigint {
  if (denom <= 0n) {
    return num;
  }
  const quotient = num / denom;
  const remainder = num % denom;
  if (remainder === 0n) {
    return quotient;
  }
  const absRemainder = remainder < 0n ? -remainder : remainder;
  const doubled = absRemainder * 2n;
  switch (rounding) {
    case "ceil":
      return num > 0n ? quotient + 1n : quotient;
    case "floor":
      return num < 0n ? quotient - 1n : quotient;
    case "half_even":
      if (doubled < denom) return quotient;
      if (doubled > denom) return num > 0n ? quotient + 1n : quotient - 1n;
      return quotient % 2n === 0n ? quotient : quotient + (num > 0n ? 1n : -1n);
    case "half_up":
    default:
      return num > 0n
        ? doubled >= denom ? quotient + 1n : quotient
        : doubled >= denom ? quotient - 1n : quotient;
  }
}

/** Format an integer count of `scale`-th units back into a decimal string, trimming trailing zeros. */
export function scaledUnitsToDecimal(units: bigint, scale = DEFAULT_DECIMAL_SCALE): string {
  const normalizedScale = normalizeScale(scale);
  const negative = units < 0n;
  const absolute = negative ? -units : units;
  const scaleUnit = 10n ** BigInt(normalizedScale);
  const whole = absolute / scaleUnit;
  if (normalizedScale === 0) {
    return negative ? `-${whole}` : whole.toString();
  }
  const fraction = String(absolute % scaleUnit).padStart(normalizedScale, "0").replace(/0+$/u, "");
  const sign = negative ? "-" : "";
  return fraction.length > 0 ? `${sign}${whole}.${fraction}` : `${sign}${whole}`;
}

/**
 * Exact decimal addition of two strings scaled to `scale`, returning a decimal
 * string. Inputs with more fractional digits than `scale` are rounded first.
 */
export function decimalAdd(
  left: string,
  right: string,
  scale = DEFAULT_DECIMAL_SCALE,
  rounding: DecimalRounding = "half_up",
): string {
  const normalizedScale = normalizeScale(scale);
  const mode = assertRounding(rounding);
  const leftUnits = decimalToScaledUnits(left, normalizedScale, mode);
  const rightUnits = decimalToScaledUnits(right, normalizedScale, mode);
  if (leftUnits === null || rightUnits === null) {
    return "0";
  }
  return scaledUnitsToDecimal(leftUnits + rightUnits, normalizedScale);
}

/** Exact decimal subtraction of two strings scaled to `scale`. */
export function decimalSubtract(
  left: string,
  right: string,
  scale = DEFAULT_DECIMAL_SCALE,
  rounding: DecimalRounding = "half_up",
): string {
  const normalizedScale = normalizeScale(scale);
  const mode = assertRounding(rounding);
  const leftUnits = decimalToScaledUnits(left, normalizedScale, mode);
  const rightUnits = decimalToScaledUnits(right, normalizedScale, mode);
  if (leftUnits === null || rightUnits === null) {
    return "0";
  }
  return scaledUnitsToDecimal(leftUnits - rightUnits, normalizedScale);
}

/**
 * Exact decimal multiplication of two strings, returning a decimal string
 * scaled to `scale`. The exact product (at arbitrary combined precision) is
 * computed in BigInt, then rounded to `scale` with `rounding`. Billing uses
 * `("ceil", scale 6)` to mirror `micro = ceil(amount × rate × 1e6)`.
 */
export function decimalMultiply(
  left: string,
  right: string,
  scale = DEFAULT_DECIMAL_SCALE,
  rounding: DecimalRounding = "half_up",
): string {
  const normalizedScale = normalizeScale(scale);
  const mode = assertRounding(rounding);
  const leftExact = parseExact(left);
  const rightExact = parseExact(right);
  if (!leftExact || !rightExact) {
    return "0";
  }
  const product = leftExact.digits * rightExact.digits;
  const productScale = leftExact.scale + rightExact.scale;
  const offset = normalizedScale - productScale;
  let resultUnits: bigint;
  if (offset >= 0) {
    resultUnits = product * 10n ** BigInt(offset);
  } else {
    const denominator = 10n ** BigInt(-offset);
    resultUnits = roundFraction(product, denominator, mode);
  }
  return scaledUnitsToDecimal(resultUnits, normalizedScale);
}

/**
 * Exact decimal division `numerator / denominator`, returning a decimal string
 * scaled to `scale`. A divide-by-zero or malformed input yields `"0"`. The
 * quotient is extended to `scale` fractional digits; when that requires
 * dropping a remainder, `rounding` decides.
 */
export function decimalDivide(
  numerator: string,
  denominator: string,
  scale = DEFAULT_DECIMAL_SCALE,
  rounding: DecimalRounding = "half_up",
): string {
  const normalizedScale = normalizeScale(scale);
  const mode = assertRounding(rounding);
  const numExact = parseExact(numerator);
  const denExact = parseExact(denominator);
  if (!numExact || !denExact || denExact.digits === 0n) {
    return "0";
  }
  // value = num / den
  // resultUnits = numerator / denominator × 10^scale
  //            = (num.digits × 10^scale × 10^den.scale) / (den.digits × 10^num.scale)
  const numPower = normalizedScale + denExact.scale - numExact.scale;
  let resultUnits: bigint;
  if (numPower >= 0) {
    resultUnits = roundFraction(numExact.digits * 10n ** BigInt(numPower), denExact.digits, mode);
  } else {
    const denomScaled = denExact.digits * 10n ** BigInt(-numPower);
    resultUnits = roundFraction(numExact.digits, denomScaled, mode);
  }
  return scaledUnitsToDecimal(resultUnits, normalizedScale);
}

/** Exact summation of decimal strings scaled to `scale` (a sum never needs rounding). */
export function sumDecimalStrings(values: string[], scale = DEFAULT_DECIMAL_SCALE): string {
  const normalizedScale = normalizeScale(scale);
  let total = 0n;
  for (const value of values) {
    const units = decimalToScaledUnits(value, normalizedScale, "floor");
    if (units !== null) {
      total += units;
    }
  }
  return scaledUnitsToDecimal(total, normalizedScale);
}