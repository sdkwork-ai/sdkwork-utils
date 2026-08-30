import assert from "node:assert/strict";
import test from "node:test";
import {
  decimalAdd,
  decimalDivide,
  decimalMultiply,
  decimalSubtract,
  decimalToScaledUnits,
  scaledUnitsToDecimal,
  sumDecimalStrings,
} from "../decimal_math.js";

test("decimalToScaledUnits / scaledUnitsToDecimal round-trip exact decimals", () => {
  assert.equal(decimalToScaledUnits("1.5", 6), 1_500_000n);
  assert.equal(decimalToScaledUnits("0.000704", 12), 704_000_000n);
  assert.equal(scaledUnitsToDecimal(1_500_000n, 6), "1.5");
  assert.equal(scaledUnitsToDecimal(704_000_000n, 12), "0.000704");
});

test("decimalToScaledUnits returns null for malformed input", () => {
  assert.equal(decimalToScaledUnits(""), null);
  assert.equal(decimalToScaledUnits("abc"), null);
  assert.equal(decimalToScaledUnits("1.2.3"), null);
});

test("decimalAdd avoids double-precision drift", () => {
  // 0.1 + 0.2 would be 0.30000000000000004 in IEEE-754 doubles.
  assert.equal(decimalAdd("0.1", "0.2", 12), "0.3");
  assert.equal(decimalAdd("10.000004", "0.000001", 12), "10.000005");
});

test("decimalSubtract is exact", () => {
  assert.equal(decimalSubtract("1", "0.000001", 12), "0.999999");
  assert.equal(decimalSubtract("1.00", "1", 12), "0");
});

test("decimalMultiply converts cash × rate into micro-points with ceil", () => {
  // Billing formula: micro = ceil(amount × rate × 1e6).
  const micro = (amount: string, rate: string): bigint =>
    decimalToScaledUnits(decimalMultiply(amount, rate, 6, "ceil"), 6) ?? 0n;
  assert.equal(micro("0.000704", "70"), 49_280n);
  assert.equal(micro("1.23", "6.96"), 8_560_800n); // exact: 8.5608 points
  // Fractional product ceil: 0.0000001 × 1e6 = 0.1 micro -> 1 micro.
  assert.equal(decimalMultiply("0.0000001", "1", 6, "ceil"), "0.000001");
  assert.equal(micro("0.0000001", "1"), 1n);
  // rate with 12 fractional digits still multiplies exactly.
  assert.equal(micro("20.00", "75.5"), 1_510_000_000n);
});

test("decimalDivide computes a scale-12 rate (points / cost) with ceil", () => {
  // 0.049280 points / 0.000704 = exactly 70.
  assert.equal(decimalDivide("0.049280", "0.000704", 12, "ceil"), "70");
  // 1 / 3 at scale 6, ceil = 0.333334.
  assert.equal(decimalDivide("1", "3", 6, "ceil"), "0.333334");
  assert.equal(decimalDivide("1", "3", 6, "floor"), "0.333333");
  assert.equal(decimalDivide("1", "3", 6, "half_up"), "0.333333");
  assert.equal(decimalDivide("1", "0", 12), "0");
});

test("rounding modes: half_up and half_even", () => {
  // 2.5 rounds away from zero for half_up.
  assert.equal(decimalMultiply("2.5", "1", 6, "half_up"), "2.5");
  assert.equal(decimalDivide("5", "2", 0, "half_up"), "3");
  // 2.5 -> 2 (even), 3.5 -> 4 (even) for half_even.
  assert.equal(decimalDivide("5", "2", 0, "half_even"), "2");
  assert.equal(decimalDivide("7", "2", 0, "half_even"), "4");
});

test("sumDecimalStrings adds a page of costs exactly", () => {
  assert.equal(sumDecimalStrings(["0.000704", "0.000001", "1.5"], 6), "1.500705");
  assert.equal(sumDecimalStrings([], 6), "0");
  assert.equal(sumDecimalStrings(["bad", "1"], 6), "1");
});

test("decimalToScaledUnits rounds excess fractional digits at the scale boundary", () => {
  // More fractional digits than scale: round the tail, not truncate-and-mis-shift.
  assert.equal(decimalToScaledUnits("0.12345678", 4, "ceil"), 1235n);
  assert.equal(decimalToScaledUnits("0.12345678", 4, "floor"), 1234n);
  assert.equal(decimalToScaledUnits("12.9999999", 6, "ceil"), 13_000_000n);
  assert.equal(decimalToScaledUnits("12.9999999", 6, "floor"), 12_999_999n);
});

test("price-rate round-trip: points/cost × cost recovery and cash×rate parity", () => {
  // Rate = ceil(points / cost, scale=12); cash×rate (ceil, scale 6) must
  // recover at least the billed micro-points (merchant never shortchanged).
  const rate = decimalDivide("0.049280", "0.000704", 12, "ceil"); // "70"
  assert.equal(rate, "70");
  const recoveredMicro = decimalToScaledUnits(decimalMultiply("0.000704", rate, 6, "ceil"), 6);
  assert.equal(recoveredMicro, 49_280n); // exactly the billed micro-points
});