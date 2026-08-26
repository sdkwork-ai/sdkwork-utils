//! HTTP JSON 边界的 int64-as-string 透明 serde helper。
//!
//! 遵循 `sdkwork-specs/API_SPEC.md` 与 `SDK_SPEC.md`：
//! - Rust 内部领域模型保持原生 `i64`。
//! - 浏览器 / TypeScript SDK / OpenAPI JSON 边界必须用字符串表示 int64，
//!   避免 JavaScript 精度丢失。
//!
//! 用法：
//! ```ignore
//! # use serde::{Deserialize, Serialize};
//! #[derive(Serialize, Deserialize)]
//! struct SitePage {
//!     #[serde(with = "sdkwork_utils_rust::serde_int64")]
//!     pub total: i64,
//! }
//! ```
//!
//! 反序列化时严格校验十进制字符串，空串与包含非数字字符的输入会被拒绝。

use serde::{Deserialize, Deserializer, Serializer};

const ERR_EMPTY: &str = "int64 string must not be empty";
const ERR_INVALID: &str = "int64 string must match ^-?[0-9]+$";
const ERR_OUT_OF_RANGE: &str = "int64 string out of i64 range";

/// 序列化 `i64` → 十进制字符串。
pub fn serialize<S>(value: &i64, serializer: S) -> Result<S::Ok, S::Error>
where
    S: Serializer,
{
    serializer.collect_str(value)
}

/// 反序列化十进制字符串 → `i64`，严格校验格式与范围。
pub fn deserialize<'de, D>(deserializer: D) -> Result<i64, D::Error>
where
    D: Deserializer<'de>,
{
    let raw = String::deserialize(deserializer)?;
    parse_strict(&raw).map_err(serde::de::Error::custom)
}

/// HTTP query 边界的 `Option<i64>` helper：空串视为 `None`，避免
/// `operator_id=` 之类的前端/代理占位参数触发 Query 提取 40002。
pub mod option_query {
    use super::parse_strict;
    use serde::de::{self, Unexpected, Visitor};
    use serde::{Deserializer, Serializer};
    use std::fmt;

    pub fn serialize<S>(value: &Option<i64>, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        match value {
            Some(v) => serializer.collect_str(v),
            None => serializer.serialize_none(),
        }
    }

    pub fn deserialize<'de, D>(deserializer: D) -> Result<Option<i64>, D::Error>
    where
        D: Deserializer<'de>,
    {
        struct OptI64QueryVisitor;

        impl Visitor<'_> for OptI64QueryVisitor {
            type Value = Option<i64>;

            fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
                formatter.write_str("an optional signed integer query parameter")
            }

            fn visit_none<E>(self) -> Result<Self::Value, E>
            where
                E: de::Error,
            {
                Ok(None)
            }

            fn visit_unit<E>(self) -> Result<Self::Value, E>
            where
                E: de::Error,
            {
                Ok(None)
            }

            fn visit_i64<E>(self, value: i64) -> Result<Self::Value, E>
            where
                E: de::Error,
            {
                Ok(Some(value))
            }

            fn visit_u64<E>(self, value: u64) -> Result<Self::Value, E>
            where
                E: de::Error,
            {
                i64::try_from(value)
                    .map(Some)
                    .map_err(|_| E::invalid_value(Unexpected::Unsigned(value), &self))
            }

            fn visit_str<E>(self, value: &str) -> Result<Self::Value, E>
            where
                E: de::Error,
            {
                let trimmed = value.trim();
                if trimmed.is_empty() {
                    return Ok(None);
                }
                parse_strict(trimmed).map(Some).map_err(E::custom)
            }
        }

        deserializer.deserialize_any(OptI64QueryVisitor)
    }
}

/// `Option<i64>` 的伴生 helper：序列化 / 反序列化 None 与 Some(i64)。
pub mod option {
    use super::parse_strict;
    use serde::de::Error as _;
    use serde::{Deserialize, Deserializer, Serializer};

    pub fn serialize<S>(value: &Option<i64>, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        match value {
            Some(v) => serializer.collect_str(v),
            None => serializer.serialize_none(),
        }
    }

    pub fn deserialize<'de, D>(deserializer: D) -> Result<Option<i64>, D::Error>
    where
        D: Deserializer<'de>,
    {
        let raw: Option<String> = Option::deserialize(deserializer)?;
        match raw {
            None => Ok(None),
            Some(text) => parse_strict(&text).map(Some).map_err(D::Error::custom),
        }
    }
}

/// 严格解析十进制字符串为 `i64`，支持负号；空串与非数字字符一律拒绝。
fn parse_strict(raw: &str) -> Result<i64, &'static str> {
    if raw.is_empty() {
        return Err(ERR_EMPTY);
    }

    let bytes = raw.as_bytes();
    let digits = match bytes.first() {
        Some(b'-') => &bytes[1..],
        _ => bytes,
    };

    if digits.is_empty() || !digits.iter().all(|b| b.is_ascii_digit()) {
        return Err(ERR_INVALID);
    }

    raw.parse::<i64>().map_err(|_| ERR_OUT_OF_RANGE)
}

#[cfg(test)]
mod tests {
    use serde::{Deserialize, Serialize};

    #[derive(Serialize, Deserialize, PartialEq, Debug)]
    struct Wrapper {
        #[serde(with = "super")]
        value: i64,
    }

    #[derive(Serialize, Deserialize, PartialEq, Debug)]
    struct OptionalWrapper {
        #[serde(with = "super::option", default)]
        value: Option<i64>,
    }

    #[test]
    fn serializes_positive_as_decimal_string() {
        let wrapper = Wrapper { value: 123_456 };
        let json = serde_json::to_string(&wrapper).unwrap();
        assert_eq!(json, r#"{"value":"123456"}"#);
    }

    #[test]
    fn serializes_negative_as_decimal_string() {
        let wrapper = Wrapper { value: -42 };
        let json = serde_json::to_string(&wrapper).unwrap();
        assert_eq!(json, r#"{"value":"-42"}"#);
    }

    #[test]
    fn serializes_max_int64_as_decimal_string() {
        let wrapper = Wrapper { value: i64::MAX };
        let json = serde_json::to_string(&wrapper).unwrap();
        assert_eq!(json, r#"{"value":"9223372036854775807"}"#);
    }

    #[test]
    fn deserializes_decimal_string_to_i64() {
        let json = r#"{"value":"9876543210"}"#;
        let wrapper: Wrapper = serde_json::from_str(json).unwrap();
        assert_eq!(wrapper.value, 9_876_543_210);
    }

    #[test]
    fn deserializes_negative_string() {
        let json = r#"{"value":"-99"}"#;
        let wrapper: Wrapper = serde_json::from_str(json).unwrap();
        assert_eq!(wrapper.value, -99);
    }

    #[test]
    fn deserializes_minimum_int64_string() {
        let json = r#"{"value":"-9223372036854775808"}"#;
        let wrapper: Wrapper = serde_json::from_str(json).unwrap();
        assert_eq!(wrapper.value, i64::MIN);
    }

    #[test]
    fn rejects_leading_plus_sign() {
        let json = r#"{"value":"+99"}"#;
        let result: Result<Wrapper, _> = serde_json::from_str(json);
        assert!(result.is_err());
    }

    #[test]
    fn rejects_non_numeric_input() {
        let json = r#"{"value":"abc"}"#;
        let result: Result<Wrapper, _> = serde_json::from_str(json);
        assert!(result.is_err());
    }

    #[test]
    fn rejects_empty_string() {
        let json = r#"{"value":""}"#;
        let result: Result<Wrapper, _> = serde_json::from_str(json);
        assert!(result.is_err());
    }

    #[test]
    fn rejects_overflow_value() {
        let json = r#"{"value":"9223372036854775808"}"#;
        let result: Result<Wrapper, _> = serde_json::from_str(json);
        assert!(result.is_err());
    }

    #[test]
    fn optional_round_trips_some() {
        let wrapper = OptionalWrapper { value: Some(42) };
        let json = serde_json::to_string(&wrapper).unwrap();
        assert_eq!(json, r#"{"value":"42"}"#);
        let parsed: OptionalWrapper = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, wrapper);
    }

    #[test]
    fn optional_round_trips_none() {
        let wrapper = OptionalWrapper { value: None };
        let json = serde_json::to_string(&wrapper).unwrap();
        assert_eq!(json, r#"{"value":null}"#);
        let parsed: OptionalWrapper = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, wrapper);
    }

    #[derive(Serialize, Deserialize, PartialEq, Debug)]
    struct OptionalQueryWrapper {
        #[serde(with = "super::option_query", default)]
        value: Option<i64>,
    }

    #[test]
    fn optional_query_treats_empty_string_as_none() {
        let parsed: OptionalQueryWrapper =
            serde_urlencoded::from_str("value=").expect("empty query value");
        assert_eq!(parsed.value, None);
    }

    #[test]
    fn optional_query_parses_decimal_string() {
        let parsed: OptionalQueryWrapper =
            serde_urlencoded::from_str("value=42").expect("numeric query value");
        assert_eq!(parsed.value, Some(42));
    }
}
