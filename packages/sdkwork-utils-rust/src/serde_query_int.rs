//! Query-string integers for flattened list DTOs.
//!
//! `serde_urlencoded` presents every query value as text. A DTO that flattens
//! another DTO (`#[serde(flatten)] pub list: SdkWorkListQuery`) reaches the
//! inner numeric fields through serde's internal `Content` buffer, where the
//! text form can no longer be reinterpreted as an integer — the request then
//! fails with `invalid type: string "2", expected i32`. That made paging on
//! every wrapper DTO impossible (`/mcp/invocations?page=2`, the Skills
//! installation and category lists).
//!
//! These helpers accept both the numeric and the text form, so a flattened
//! list DTO pages exactly like a top-level one. Rejecting junk is unchanged:
//! a value that is neither a number nor a decimal string still fails the
//! request with a 400 at the boundary.
//!
//! Use them through the full module path, like the other `serde_*` helpers:
//! `#[serde(default, deserialize_with = "sdkwork_utils_rust::serde_query_int::option_i32")]`.

use serde::de::{self, Unexpected, Visitor};
use serde::{Deserialize, Deserializer};
use std::fmt;

/// Deserializes an optional `i32` from a JSON/query number or a decimal string.
pub fn option_i32<'de, D>(deserializer: D) -> Result<Option<i32>, D::Error>
where
    D: Deserializer<'de>,
{
    deserializer.deserialize_option(OptionalIntVisitor)
}

/// Deserializes an optional `u64` from a JSON/query number or a decimal string.
pub fn option_u64<'de, D>(deserializer: D) -> Result<Option<u64>, D::Error>
where
    D: Deserializer<'de>,
{
    deserializer.deserialize_option(OptionalUnsignedVisitor)
}

/// Deserializes an `i32` from a JSON/query number or a decimal string.
pub fn i32<'de, D>(deserializer: D) -> Result<i32, D::Error>
where
    D: Deserializer<'de>,
{
    let value = Option::<i64>::deserialize(deserializer)?;
    value
        .ok_or_else(|| de::Error::invalid_type(Unexpected::Option, &"a decimal integer"))
        .and_then(|number| {
            i32::try_from(number)
                .map_err(|_| de::Error::custom(format!("{number} is outside the i32 range")))
        })
}

struct OptionalIntVisitor;

impl<'de> Visitor<'de> for OptionalIntVisitor {
    type Value = Option<i32>;

    fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("an optional integer or a decimal integer string")
    }

    fn visit_none<E: de::Error>(self) -> Result<Self::Value, E> {
        Ok(None)
    }

    fn visit_unit<E: de::Error>(self) -> Result<Self::Value, E> {
        Ok(None)
    }

    fn visit_some<D>(self, deserializer: D) -> Result<Self::Value, D::Error>
    where
        D: Deserializer<'de>,
    {
        deserializer.deserialize_any(IntValueVisitor).map(Some)
    }
}

struct IntValueVisitor;

impl<'de> Visitor<'de> for IntValueVisitor {
    type Value = i32;

    fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("an integer or a decimal integer string")
    }

    fn visit_i64<E: de::Error>(self, value: i64) -> Result<Self::Value, E> {
        i32::try_from(value)
            .map_err(|_| de::Error::custom(format!("{value} is outside the i32 range")))
    }

    fn visit_u64<E: de::Error>(self, value: u64) -> Result<Self::Value, E> {
        i32::try_from(value)
            .map_err(|_| de::Error::custom(format!("{value} is outside the i32 range")))
    }

    fn visit_f64<E: de::Error>(self, value: f64) -> Result<Self::Value, E> {
        if value.fract() != 0.0 {
            return Err(de::Error::custom(format!("{value} is not a whole number")));
        }
        i32::try_from(value as i64)
            .map_err(|_| de::Error::custom(format!("{value} is outside the i32 range")))
    }

    fn visit_str<E: de::Error>(self, value: &str) -> Result<Self::Value, E> {
        value
            .trim()
            .parse::<i32>()
            .map_err(|_| de::Error::invalid_value(Unexpected::Str(value), &self))
    }
}

struct OptionalUnsignedVisitor;

impl<'de> Visitor<'de> for OptionalUnsignedVisitor {
    type Value = Option<u64>;

    fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("an optional non-negative integer or a decimal integer string")
    }

    fn visit_none<E: de::Error>(self) -> Result<Self::Value, E> {
        Ok(None)
    }

    fn visit_unit<E: de::Error>(self) -> Result<Self::Value, E> {
        Ok(None)
    }

    fn visit_some<D>(self, deserializer: D) -> Result<Self::Value, D::Error>
    where
        D: Deserializer<'de>,
    {
        deserializer.deserialize_any(UnsignedValueVisitor).map(Some)
    }
}

struct UnsignedValueVisitor;

impl<'de> Visitor<'de> for UnsignedValueVisitor {
    type Value = u64;

    fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("a non-negative integer or a decimal integer string")
    }

    fn visit_u64<E: de::Error>(self, value: u64) -> Result<Self::Value, E> {
        Ok(value)
    }

    fn visit_i64<E: de::Error>(self, value: i64) -> Result<Self::Value, E> {
        u64::try_from(value).map_err(|_| de::Error::custom(format!("{value} is negative")))
    }

    fn visit_str<E: de::Error>(self, value: &str) -> Result<Self::Value, E> {
        value
            .trim()
            .parse::<u64>()
            .map_err(|_| de::Error::invalid_value(Unexpected::Str(value), &self))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde::Deserialize;

    #[derive(Debug, Deserialize)]
    struct Nested {
        #[serde(default, deserialize_with = "option_i32")]
        page: Option<i32>,
    }

    #[derive(Debug, Deserialize)]
    struct Wrapper {
        #[serde(flatten)]
        list: Nested,
        subject_id: Option<u64>,
    }

    #[test]
    fn flattened_optional_int_accepts_the_query_string_form() {
        let wrapper: Wrapper = serde_urlencoded::from_str("page=2&subject_id=7")
            .expect("flattened numeric query parses");
        assert_eq!(wrapper.list.page, Some(2));
        assert_eq!(wrapper.subject_id, Some(7));
    }

    #[test]
    fn flattened_optional_int_accepts_a_json_number() {
        let nested: Nested = serde_json::from_str(r#"{"page":3}"#).expect("json parses");
        assert_eq!(nested.page, Some(3));
    }

    #[test]
    fn flattened_optional_int_keeps_absent_values_absent() {
        let wrapper: Wrapper =
            serde_urlencoded::from_str("subject_id=7").expect("query without page parses");
        assert_eq!(wrapper.list.page, None);
    }

    #[test]
    fn flattened_optional_int_rejects_junk() {
        assert!(serde_urlencoded::from_str::<Wrapper>("page=abc").is_err());
        assert!(serde_urlencoded::from_str::<Wrapper>("page=9999999999999").is_err());
    }

    #[test]
    fn optional_unsigned_accepts_the_query_string_form() {
        #[derive(Debug, Deserialize)]
        struct UnsignedWrapper {
            #[serde(default, deserialize_with = "option_u64")]
            subject_id: Option<u64>,
        }

        let wrapper: UnsignedWrapper =
            serde_urlencoded::from_str("subject_id=42").expect("unsigned query parses");
        assert_eq!(wrapper.subject_id, Some(42));
        assert!(serde_urlencoded::from_str::<UnsignedWrapper>("subject_id=-1").is_err());
    }
}
