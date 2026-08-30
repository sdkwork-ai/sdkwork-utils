//! Token Bank points precision helpers.
//!
//! Token Bank points carry up to 6 decimal places so fractional usage debits
//! and account balances stay exact. Internally the ledger and database store
//! those points as integer **micro-points** (1 point = 1_000_000 micro points)
//! so a single integer numeric unit is shared everywhere and no floating point
//! drift is ever introduced.
//!
//! These helpers convert between the integer micro representation and a
//! 6-decimal string used at parse/display boundaries. All arithmetic is done
//! on integer strings so results are exact for arbitrary input magnitudes.

/// Number of decimal digits a Token Bank point carries.
pub const TOKEN_POINTS_SCALE: u8 = 6;

/// Micro-points that make up a single whole point.
pub const MICRO_POINTS_PER_POINT: i64 = 1_000_000;

/// Convert a non-negative micro-point integer into a points decimal string
/// with up to 6 fractional digits. Trailing fractional zeros are trimmed.
///
/// ```
/// use sdkwork_utils_rust::token_bank::{micro_to_decimal_string, MICRO_POINTS_PER_POINT};
/// assert_eq!(micro_to_decimal_string(0), "0");
/// assert_eq!(micro_to_decimal_string(MICRO_POINTS_PER_POINT), "1");
/// assert_eq!(micro_to_decimal_string(MICRO_POINTS_PER_POINT / 2), "0.5");
/// assert_eq!(micro_to_decimal_string(12 * MICRO_POINTS_PER_POINT + 1), "12.000001");
/// ```
pub fn micro_to_decimal_string(micro: i64) -> String {
    if micro < 0 {
        return "0".to_string();
    }
    let whole = micro / MICRO_POINTS_PER_POINT;
    let fraction = micro % MICRO_POINTS_PER_POINT;
    if fraction == 0 {
        return whole.to_string();
    }
    let mut frac_digits = format!("{fraction:06}");
    while frac_digits.ends_with('0') {
        frac_digits.pop();
    }
    format!("{whole}.{frac_digits}")
}

/// Parse a points decimal string into an integer micro-point value.
///
/// The value must be a non-negative decimal with at most 6 fractional digits
/// (`12`, `12.5`, `12.000001`). Returns `None` for negatives, malformed
/// input, or values that do not fit a 64-bit signed integer.
///
/// ```
/// use sdkwork_utils_rust::token_bank::decimal_string_to_micro;
/// assert_eq!(decimal_string_to_micro("0"), Some(0));
/// assert_eq!(decimal_string_to_micro("1.5"), Some(1_500_000));
/// assert_eq!(decimal_string_to_micro("12.000001"), Some(12_000_001));
/// assert_eq!(decimal_string_to_micro("1."), None);
/// assert_eq!(decimal_string_to_micro("-1"), None);
/// assert_eq!(decimal_string_to_micro("1.1234567"), None);
/// ```
pub fn decimal_string_to_micro(value: &str) -> Option<i64> {
    let value = value.trim();
    if value.is_empty() || value.starts_with('-') || value.starts_with('+') {
        return None;
    }
    let mut parts = value.split('.');
    let whole = parts.next()?;
    let fraction = parts.next();
    if parts.next().is_some() {
        return None;
    }

    // Whole part: digits only (allow any length, strip leading zeros).
    let whole_ok = !whole.is_empty() && whole.chars().all(|ch| ch.is_ascii_digit());
    if !whole_ok {
        return None;
    }

    // Fraction part: optional, digits only, at most 6 digits; a trailing '.'
    // like "1." is rejected.
    let fraction_digits: &str = match fraction {
        Some(f) => {
            if f.is_empty()
                || f.len() > TOKEN_POINTS_SCALE as usize
                || !f.chars().all(|ch| ch.is_ascii_digit())
            {
                return None;
            }
            f
        }
        None => "",
    };

    let whole_trimmed = whole.trim_start_matches('0');
    let whole_value: i64 = if whole_trimmed.is_empty() {
        0
    } else {
        whole_trimmed.parse().ok()?
    };

    let mut micro: i128 = (whole_value as i128) * (MICRO_POINTS_PER_POINT as i128);
    if !fraction_digits.is_empty() {
        let pad = (TOKEN_POINTS_SCALE as usize) - fraction_digits.len();
        let scaled: i128 = format!("{fraction_digits}{}", "0".repeat(pad)).parse().ok()?;
        micro += scaled;
    }

    i64::try_from(micro).ok()
}

/// Round a points decimal string **up** to the nearest micro-point, returning
/// the integer micro value. Used by settlement so a fractional charge always
/// debits the merchant with at least the full owed amount (never a fraction
/// shortfall), guaranteeing no merchant loss.
///
/// ```
/// use sdkwork_utils_rust::token_bank::decimal_string_to_micro_ceil;
/// assert_eq!(decimal_string_to_micro_ceil("0.000001"), Some(1));
/// assert_eq!(decimal_string_to_micro_ceil("1.2"), Some(1_200_000));
/// assert_eq!(decimal_string_to_micro_ceil("1.000001"), Some(1_000_001));
/// ```
pub fn decimal_string_to_micro_ceil(value: &str) -> Option<i64> {
    decimal_string_to_micro(value)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn micro_to_decimal() {
        assert_eq!(micro_to_decimal_string(0), "0");
        assert_eq!(micro_to_decimal_string(1), "0.000001");
        assert_eq!(micro_to_decimal_string(MICRO_POINTS_PER_POINT), "1");
        assert_eq!(micro_to_decimal_string(MICRO_POINTS_PER_POINT / 2), "0.5");
        assert_eq!(micro_to_decimal_string(12 * MICRO_POINTS_PER_POINT + 1), "12.000001");
        assert_eq!(micro_to_decimal_string(123 * MICRO_POINTS_PER_POINT), "123");
    }

    #[test]
    fn decimal_to_micro() {
        assert_eq!(decimal_string_to_micro("0"), Some(0));
        assert_eq!(decimal_string_to_micro("00"), Some(0));
        assert_eq!(decimal_string_to_micro("1"), Some(1_000_000));
        assert_eq!(decimal_string_to_micro("1.5"), Some(1_500_000));
        assert_eq!(decimal_string_to_micro("12.000001"), Some(12_000_001));
        assert_eq!(decimal_string_to_micro("0.000001"), Some(1));
        assert_eq!(decimal_string_to_micro(" 1.20 "), Some(1_200_000));
        assert_eq!(decimal_string_to_micro(""), None);
        assert_eq!(decimal_string_to_micro("."), None);
        assert_eq!(decimal_string_to_micro("1."), None);
        assert_eq!(decimal_string_to_micro(".5"), None);
        assert_eq!(decimal_string_to_micro("1a"), None);
        assert_eq!(decimal_string_to_micro("1.2.3"), None);
        assert_eq!(decimal_string_to_micro("-1"), None);
        assert_eq!(decimal_string_to_micro("1.1234567"), None);
        assert_eq!(decimal_string_to_micro_ceil("1.2"), Some(1_200_000));
        assert_eq!(decimal_string_to_micro_ceil("1.000001"), Some(1_000_001));
    }

    #[test]
    fn round_trip() {
        for value in ["0", "1", "1.5", "0.000001", "12.000001", "99.999999", "123456.78901"] {
            let micro = decimal_string_to_micro(value).unwrap();
            assert_eq!(micro_to_decimal_string(micro), value, "round-trip {value}");
        }
    }
}