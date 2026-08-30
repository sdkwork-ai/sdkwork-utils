//! Exact decimal arithmetic on integer units.
//!
//! Mirror of the TypeScript `@sdkwork/utils` `decimal_math` module so the same
//! add / subtract / multiply / divide never loses a fraction on either side of
//! the wire. A decimal string is parsed into an integer count of `scale`-th
//! units (scale 6 => `1.5` -> `1500000`), the operation is performed in exact
//! `i128` integer arithmetic (checked so overflow is a hard error rather than
//! silent corruption), a rounding mode is applied only when a scaling boundary
//! is crossed, and the result is formatted back to a plain decimal string.
//!
//! Rounding modes mirror the TypeScript contract:
//!  - `Floor` truncates toward -infinity
//!  - `Ceil` rounds toward +infinity (billing uses this so a fractional charge
//!    never shortchanges the merchant)
//!  - `HalfUp` rounds half away from zero
//!  - `HalfEven` rounds half to even (banker's rounding)

use std::fmt;

/// Signed integer unit used by the shared arithmetic. `i128` leaves generous
/// headroom for the scale-12 / scale-24 intermediates used in pricing.
pub type ScaledUnits = i128;

/// Rounding policy applied only when a scaling boundary crosses a remainder.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DecimalRounding {
    Floor,
    Ceil,
    HalfUp,
    HalfEven,
}

impl Default for DecimalRounding {
    fn default() -> Self {
        DecimalRounding::HalfUp
    }
}

/// A parsed decimal value split into integer digits and fractional length, so
/// products / quotients stay exact until a final requested-scale rounding.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct ExactValue {
    digits: ScaledUnits,
    scale: u32,
}

/// Error returned by the exact arithmetic helpers for malformed or out-of
/// `i128`-range input, or an overflowed intermediate.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DecimalMathError {
    /// Input is empty or not a valid `-digits[.digits]` decimal string.
    InvalidInput(String),
    /// An `i128` overflow happened while producing the exact result.
    Overflow,
    /// Division by zero was requested.
    DivideByZero,
}

impl fmt::Display for DecimalMathError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidInput(value) => write!(f, "invalid decimal string: {value:?}"),
            Self::Overflow => f.write_str("decimal arithmetic overflow"),
            Self::DivideByZero => f.write_str("divide by zero in decimal arithmetic"),
        }
    }
}

impl std::error::Error for DecimalMathError {}

type MathResult<T> = Result<T, DecimalMathError>;

fn scale_unit(scale: u32) -> Result<ScaledUnits, DecimalMathError> {
    10i128
        .checked_pow(scale)
        .ok_or(DecimalMathError::Overflow)
}

/// Parse a signed decimal string into the integer count of `scale`-th units.
/// A value carrying more fractional digits than `scale` is rounded to `scale`
/// with `rounding`. Returns `Err` for empty / malformed input or overflow.
pub fn decimal_to_scaled(
    value: &str,
    scale: u32,
    rounding: DecimalRounding,
) -> MathResult<ScaledUnits> {
    let head = value.trim();
    if head.is_empty() {
        return Err(DecimalMathError::InvalidInput(head.to_owned()));
    }
    let (negative, unsigned) = match head.strip_prefix('-') {
        Some(rest) => (true, rest),
        None => (false, head),
    };
    let mut dot = unsigned.splitn(2, '.');
    let whole_raw = dot.next().unwrap_or("");
    let fraction_raw = dot.next();
    if (unsigned.contains('.') && fraction_raw.is_none()) || whole_raw.is_empty() {
        return Err(DecimalMathError::InvalidInput(head.to_owned()));
    }
    if !whole_raw.chars().all(|ch| ch.is_ascii_digit()) {
        return Err(DecimalMathError::InvalidInput(head.to_owned()));
    }
    let fraction = fraction_raw.unwrap_or("");
    if !fraction.chars().all(|ch| ch.is_ascii_digit()) {
        return Err(DecimalMathError::InvalidInput(head.to_owned()));
    }
    // Exact value as an integer: (whole * 10^fraction_len) + fraction_digits,
    // i.e. the number in units of 10^-fraction_len.
    let n = fraction.len() as u32;
    let whole: ScaledUnits = whole_raw
        .parse()
        .map_err(|_| DecimalMathError::InvalidInput(head.to_owned()))?;
    let mut raw = whole
        .checked_mul(10i128.checked_pow(n).ok_or(DecimalMathError::Overflow)?)
        .ok_or(DecimalMathError::Overflow)?;
    if n > 0 {
        let frac: ScaledUnits = fraction
            .parse()
            .map_err(|_| DecimalMathError::InvalidInput(head.to_owned()))?;
        raw = raw.checked_add(frac).ok_or(DecimalMathError::Overflow)?;
    }
    // Shift into `scale`-N units, rounding only when more digits than `scale`.
    let mut units = if n <= scale {
        raw.checked_mul(10i128.checked_pow(scale - n).ok_or(DecimalMathError::Overflow)?)
            .ok_or(DecimalMathError::Overflow)?
    } else {
        let denom = 10i128
            .checked_pow(n - scale)
            .ok_or(DecimalMathError::Overflow)?;
        round_fraction(raw, denom, rounding)
    };
    if negative {
        units = units.checked_neg().ok_or(DecimalMathError::Overflow)?;
    }
    Ok(units)
}

/// Parse the exact value of a signed decimal string into integer digits and a
/// fractional scale without any loss. Used internally by arithmetic.
fn parse_exact(value: &str) -> MathResult<ExactValue> {
    let head = value.trim();
    if head.is_empty() {
        return Err(DecimalMathError::InvalidInput(head.to_owned()));
    }
    let (negative, unsigned) = match head.strip_prefix('-') {
        Some(rest) => (true, rest),
        None => (false, head),
    };
    let mut dot = unsigned.splitn(2, '.');
    let whole_raw = dot.next().unwrap_or("");
    let fraction_raw = dot.next();
    if (unsigned.contains('.') && fraction_raw.is_none()) || whole_raw.is_empty() {
        return Err(DecimalMathError::InvalidInput(head.to_owned()));
    }
    if !whole_raw.chars().all(|ch| ch.is_ascii_digit())
        || fraction_raw.is_some_and(|f| !f.chars().all(|ch| ch.is_ascii_digit()))
    {
        return Err(DecimalMathError::InvalidInput(head.to_owned()));
    }
    let fraction = fraction_raw.unwrap_or("");
    let trimmed = fraction.trim_end_matches('0');
    let whole: ScaledUnits = whole_raw
        .parse()
        .map_err(|_| DecimalMathError::InvalidInput(head.to_owned()))?;
    let mut digits = whole
        .checked_mul(10i128.checked_pow(trimmed.len() as u32).ok_or(DecimalMathError::Overflow)?)
        .ok_or(DecimalMathError::Overflow)?;
    if !trimmed.is_empty() {
        let frac: ScaledUnits = trimmed
            .parse()
            .map_err(|_| DecimalMathError::InvalidInput(head.to_owned()))?;
        digits = digits.checked_add(frac).ok_or(DecimalMathError::Overflow)?;
    }
    if negative {
        digits = digits.checked_neg().ok_or(DecimalMathError::Overflow)?;
    }
    Ok(ExactValue {
        digits,
        scale: trimmed.len() as u32,
    })
}

/// Round `num / denom` to an integer using `rounding` (denom must be positive).
pub fn round_fraction(num: ScaledUnits, denom: ScaledUnits, rounding: DecimalRounding) -> ScaledUnits {
    if denom <= 0 {
        return num;
    }
    let quotient = num / denom;
    let remainder = num % denom;
    if remainder == 0 {
        return quotient;
    }
    let abs_remainder = remainder.unsigned_abs();
    let doubled = abs_remainder.saturating_mul(2);
    let positive = num > 0;
    match rounding {
        DecimalRounding::Ceil => {
            if positive {
                quotient + 1
            } else {
                quotient
            }
        }
        DecimalRounding::Floor => {
            if positive {
                quotient
            } else {
                quotient - 1
            }
        }
        DecimalRounding::HalfEven => {
            if doubled < denom.unsigned_abs() {
                quotient
            } else if doubled > denom.unsigned_abs() {
                if positive {
                    quotient + 1
                } else {
                    quotient - 1
                }
            } else if quotient % 2 == 0 {
                quotient
            } else if positive {
                quotient + 1
            } else {
                quotient - 1
            }
        }
        DecimalRounding::HalfUp => {
            if doubled >= denom.unsigned_abs() {
                if positive {
                    quotient + 1
                } else {
                    quotient - 1
                }
            } else {
                quotient
            }
        }
    }
}

/// Format an integer count of `scale`-th units back into a decimal string,
/// trimming trailing fractional zeros.
pub fn scaled_to_decimal(units: ScaledUnits, scale: u32) -> MathResult<String> {
    let unit = scale_unit(scale)?;
    let negative = units < 0;
    let absolute = units.unsigned_abs();
    let whole = (absolute / unit as u128) as ScaledUnits;
    if scale == 0 {
        return Ok(if negative {
            format!("-{whole}")
        } else {
            whole.to_string()
        });
    }
    let fraction_raw = format!("{:0width$}", absolute % unit as u128, width = scale as usize);
    let fraction = fraction_raw.trim_end_matches('0');
    let sign = if negative { "-" } else { "" };
    if fraction.is_empty() {
        Ok(format!("{sign}{whole}"))
    } else {
        Ok(format!("{sign}{whole}.{fraction}"))
    }
}

fn add_scaled(left: &str, right: &str, scale: u32, rounding: DecimalRounding, negate_right: bool) -> MathResult<String> {
    let unit = scale_unit(scale)?;
    let mut left_units = decimal_to_scaled(left, scale, rounding)?;
    let right_units = decimal_to_scaled(right, scale, rounding)?;
    left_units = if negate_right {
        left_units
            .checked_sub(right_units)
            .ok_or(DecimalMathError::Overflow)?
    } else {
        left_units
            .checked_add(right_units)
            .ok_or(DecimalMathError::Overflow)?
    };
    scaled_to_decimal(left_units, scale)
        .map_err(|_| DecimalMathError::Overflow)
        .map(|v| {
            let _ = unit;
            v
        })
}

/// Exact decimal addition of two strings scaled to `scale`.
pub fn decimal_add(left: &str, right: &str, scale: u32, rounding: DecimalRounding) -> MathResult<String> {
    add_scaled(left, right, scale, rounding, false)
}

/// Exact decimal subtraction of two strings scaled to `scale`.
pub fn decimal_subtract(left: &str, right: &str, scale: u32, rounding: DecimalRounding) -> MathResult<String> {
    add_scaled(left, right, scale, rounding, true)
}

/// Exact decimal multiplication of two strings, returning a decimal string
/// scaled to `scale`. The exact product is computed in integer arithmetic, then
/// rounded to `scale` with `rounding`. Billing calls `(scale 6, Ceil)`.
pub fn decimal_multiply(left: &str, right: &str, scale: u32, rounding: DecimalRounding) -> MathResult<String> {
    let left_exact = parse_exact(left)?;
    let right_exact = parse_exact(right)?;
    let product = left_exact
        .digits
        .checked_mul(right_exact.digits)
        .ok_or(DecimalMathError::Overflow)?;
    let product_scale = left_exact.scale + right_exact.scale;
    let offset = scale as i32 - product_scale as i32;
    let result_units = if offset >= 0 {
        product
            .checked_mul(10i128.checked_pow(offset as u32).ok_or(DecimalMathError::Overflow)?)
            .ok_or(DecimalMathError::Overflow)?
    } else {
        let denom = 10i128
            .checked_pow((-offset) as u32)
            .ok_or(DecimalMathError::Overflow)?;
        round_fraction(product, denom, rounding)
    };
    scaled_to_decimal(result_units, scale)
}

/// Exact decimal division `numerator / denominator`, returning a decimal string
/// scaled to `scale`. Divide-by-zero or malformed input is an error.
pub fn decimal_divide(numerator: &str, denominator: &str, scale: u32, rounding: DecimalRounding) -> MathResult<String> {
    let num_exact = parse_exact(numerator)?;
    let den_exact = parse_exact(denominator)?;
    if den_exact.digits == 0 {
        return Err(DecimalMathError::DivideByZero);
    }
    let num_power = scale as i32 + den_exact.scale as i32 - num_exact.scale as i32;
    let result_units = if num_power >= 0 {
        let scaled =
            num_exact
                .digits
                .checked_mul(10i128.checked_pow(num_power as u32).ok_or(DecimalMathError::Overflow)?)
                .ok_or(DecimalMathError::Overflow)?;
        round_fraction(scaled, den_exact.digits, rounding)
    } else {
        let den_scaled = den_exact
            .digits
            .checked_mul(10i128.checked_pow((-num_power) as u32).ok_or(DecimalMathError::Overflow)?)
            .ok_or(DecimalMathError::Overflow)?;
        round_fraction(num_exact.digits, den_scaled, rounding)
    };
    scaled_to_decimal(result_units, scale)
}

/// Exact summation of decimal strings scaled to `scale` (a sum never needs
/// rounding; malformed entries are counted as zero).
pub fn sum_decimal_strings(values: &[String], scale: u32) -> MathResult<String> {
    let unit = scale_unit(scale)?;
    let mut total = 0i128;
    for value in values {
        if let Ok(units) = decimal_to_scaled(value, scale, DecimalRounding::Floor) {
            total += units;
        }
    }
    scaled_to_decimal(total, scale).or_else(|_| {
        let _ = unit;
        Err(DecimalMathError::Overflow)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn add(a: &str, b: &str) -> String {
        decimal_add(a, b, 12, DecimalRounding::HalfUp).unwrap()
    }

    fn mul(a: &str, b: &str) -> i128 {
        decimal_to_scaled(
            &decimal_multiply(a, b, 6, DecimalRounding::Ceil).unwrap(),
            6,
            DecimalRounding::Floor,
        )
        .unwrap()
    }

    #[test]
    fn scaled_round_trip_exact() {
        assert_eq!(decimal_to_scaled("1.5", 6, DecimalRounding::Floor).unwrap(), 1_500_000);
        assert_eq!(
            decimal_to_scaled("0.000704", 12, DecimalRounding::Floor).unwrap(),
            704_000_000
        );
        assert_eq!(scaled_to_decimal(1_500_000, 6).unwrap(), "1.5");
        assert_eq!(scaled_to_decimal(704_000_000, 12).unwrap(), "0.000704");
    }

    #[test]
    fn invalid_input_is_error() {
        assert!(decimal_to_scaled("", 6, DecimalRounding::Floor).is_err());
        assert!(decimal_to_scaled("abc", 6, DecimalRounding::Floor).is_err());
        assert!(decimal_to_scaled("1.2.3", 6, DecimalRounding::Floor).is_err());
    }

    #[test]
    fn add_avoids_float_drift() {
        assert_eq!(add("0.1", "0.2"), "0.3");
        assert_eq!(add("10.000004", "0.000001"), "10.000005");
    }

    #[test]
    fn subtract_is_exact() {
        assert_eq!(decimal_subtract("1", "0.000001", 12, DecimalRounding::HalfUp).unwrap(), "0.999999");
        assert_eq!(decimal_subtract("1.00", "1", 12, DecimalRounding::HalfUp).unwrap(), "0");
    }

    #[test]
    fn multiply_converts_cash_times_rate_into_micro_with_ceil() {
        assert_eq!(mul("0.000704", "70"), 49_280);
        assert_eq!(mul("1.23", "6.96"), 8_560_800);
        assert_eq!(mul("0.0000001", "1"), 1);
        assert_eq!(mul("20.00", "75.5"), 1_510_000_000);
    }

    #[test]
    fn divide_computes_scale_12_rate_with_ceil() {
        assert_eq!(
            decimal_divide("0.049280", "0.000704", 12, DecimalRounding::Ceil).unwrap(),
            "70"
        );
        assert_eq!(decimal_divide("1", "3", 6, DecimalRounding::Ceil).unwrap(), "0.333334");
        assert_eq!(decimal_divide("1", "3", 6, DecimalRounding::Floor).unwrap(), "0.333333");
        assert!(decimal_divide("1", "0", 12, DecimalRounding::HalfUp).is_err());
    }

    #[test]
    fn to_scaled_rounds_excess_fraction_digits() {
        assert_eq!(
            decimal_to_scaled("0.12345678", 4, DecimalRounding::Ceil).unwrap(),
            1235
        );
        assert_eq!(
            decimal_to_scaled("0.12345678", 4, DecimalRounding::Floor).unwrap(),
            1234
        );
        assert_eq!(
            decimal_to_scaled("12.9999999", 6, DecimalRounding::Ceil).unwrap(),
            13_000_000
        );
    }

    #[test]
    fn rounding_modes() {
        assert_eq!(decimal_divide("5", "2", 0, DecimalRounding::HalfUp).unwrap(), "3");
        assert_eq!(decimal_divide("5", "2", 0, DecimalRounding::HalfEven).unwrap(), "2");
        assert_eq!(decimal_divide("7", "2", 0, DecimalRounding::HalfEven).unwrap(), "4");
    }

    #[test]
    fn sum_page_costs_exactly() {
        assert_eq!(
            sum_decimal_strings(&["0.000704".into(), "0.000001".into(), "1.5".into()], 6).unwrap(),
            "1.500705"
        );
        assert_eq!(sum_decimal_strings(&[], 6).unwrap(), "0");
    }
}