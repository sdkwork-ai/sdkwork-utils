//! SDKWork write-command header contract shared by app-api and backend-api routes.
//!
//! Idempotent write operations (`x-sdkwork-idempotent: true`) require
//! `Idempotency-Key` and `Sdkwork-Request-Hash` at the handler layer
//! (`API_SPEC.md` section 14/15 idempotent command rules). This module is the
//! single implementation of the wire rules so both HTTP surfaces cannot drift:
//!
//! - `Idempotency-Key`: required, trimmed, 1..=128 characters matching
//!   `^[A-Za-z0-9._:-]+$` (the OpenAPI header schema).
//! - `Sdkwork-Request-Hash`: required, trimmed, non-empty; callers compare it
//!   against the canonical body hash from [`sdkwork_stable_json_request_hash`].
//! - `Sdkwork-Request-No`: optional; falls back to a caller-supplied
//!   derivation from the idempotency key.
//!
//! Callers extract raw header values from their HTTP framework and pass
//! `Option<&str>` (typically `headers.get(name).and_then(|v| v.to_str().ok())`);
//! a non-UTF-8 value therefore reports as missing, which is fail-closed.

use serde::Serialize;

pub const SDKWORK_IDEMPOTENCY_KEY_HEADER: &str = "Idempotency-Key";
pub const SDKWORK_REQUEST_HASH_HEADER: &str = "Sdkwork-Request-Hash";
pub const SDKWORK_REQUEST_NO_HEADER: &str = "Sdkwork-Request-No";

const IDEMPOTENCY_KEY_MAX_LEN: usize = 128;

/// Parsed write-command headers for one idempotent request.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SdkWorkWriteCommandHeaders {
    pub idempotency_key: String,
    pub request_hash: String,
    pub request_no: String,
}

/// Failure to parse or validate the write-command headers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SdkWorkWriteCommandHeaderError {
    MissingHeader(&'static str),
    InvalidHeader(&'static str),
}

/// Deterministic, bounded request-hash seed: sanitized scope plus parts.
#[must_use]
pub fn sdkwork_stable_command_request_hash(scope: &str, parts: &[&str]) -> String {
    let mut normalized = vec![scope];
    normalized.extend(parts.iter().copied());
    normalized
        .iter()
        .map(|part| {
            part.chars()
                .map(|character| {
                    if character.is_ascii_alphanumeric() || matches!(character, '-' | '_' | '.') {
                        character
                    } else {
                        '-'
                    }
                })
                .collect::<String>()
        })
        .collect::<Vec<_>>()
        .join("-")
}

/// Canonical body hash for a JSON payload.
///
/// # Errors
/// [`SdkWorkWriteCommandHeaderError::InvalidHeader`] when the payload cannot
/// be serialized (the request body is not a representable JSON value).
pub fn sdkwork_stable_json_request_hash(
    scope: &str,
    value: &impl Serialize,
) -> Result<String, SdkWorkWriteCommandHeaderError> {
    let value = serde_json::to_value(value).map_err(|_| {
        SdkWorkWriteCommandHeaderError::InvalidHeader(
            "request body could not be canonicalized for request hash validation",
        )
    })?;
    Ok(sdkwork_stable_canonical_json_request_hash(scope, &value))
}

/// Canonical body hash over an already-materialized JSON value.
#[must_use]
pub fn sdkwork_stable_canonical_json_request_hash(
    scope: &str,
    value: &serde_json::Value,
) -> String {
    sdkwork_stable_command_request_hash(scope, &[&canonical_json_string(value)])
}

/// Hash payload that folds a route parameter into the serialized body.
///
/// # Panics
/// Only when the caller-built body cannot serialize, which for the intended
/// `serde_json::json!({...})` inputs is impossible.
#[must_use]
pub fn sdkwork_write_payload_with_route_param(
    route_param_key: &str,
    route_param_value: &str,
    body: &impl Serialize,
) -> serde_json::Value {
    let mut payload = serde_json::to_value(body).expect("write payload must serialize");
    if let serde_json::Value::Object(ref mut fields) = payload {
        fields.insert(
            route_param_key.to_string(),
            serde_json::Value::String(route_param_value.to_string()),
        );
    }
    payload
}

/// Single shared implementation of the write-command header rules.
///
/// # Errors
/// [`SdkWorkWriteCommandHeaderError::MissingHeader`] when a required header is
/// absent, empty after trimming, or not valid UTF-8;
/// [`SdkWorkWriteCommandHeaderError::InvalidHeader`] when the idempotency key
/// violates the OpenAPI length or character contract.
pub fn parse_sdkwork_write_command_headers(
    idempotency_key: Option<&str>,
    request_hash: Option<&str>,
    request_no: Option<&str>,
    fallback_request_no: impl FnOnce(&str) -> String,
) -> Result<SdkWorkWriteCommandHeaders, SdkWorkWriteCommandHeaderError> {
    let idempotency_key = required_header(idempotency_key, SDKWORK_IDEMPOTENCY_KEY_HEADER)?;
    let idempotency_key = validate_idempotency_key(idempotency_key)?;
    let request_hash = required_header(request_hash, SDKWORK_REQUEST_HASH_HEADER)?;
    let request_no =
        header_text(request_no).unwrap_or_else(|| fallback_request_no(&idempotency_key));
    Ok(SdkWorkWriteCommandHeaders {
        idempotency_key,
        request_hash,
        request_no,
    })
}

fn required_header(
    value: Option<&str>,
    name: &'static str,
) -> Result<String, SdkWorkWriteCommandHeaderError> {
    header_text(value).ok_or(SdkWorkWriteCommandHeaderError::MissingHeader(name))
}

fn header_text(value: Option<&str>) -> Option<String> {
    value
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_owned)
}

fn validate_idempotency_key(value: String) -> Result<String, SdkWorkWriteCommandHeaderError> {
    let valid_length = (1..=IDEMPOTENCY_KEY_MAX_LEN).contains(&value.len());
    let valid_characters = value.chars().all(|character| {
        character.is_ascii_alphanumeric() || matches!(character, '.' | '_' | ':' | '-')
    });
    if valid_length && valid_characters {
        Ok(value)
    } else {
        Err(SdkWorkWriteCommandHeaderError::InvalidHeader(
            "Idempotency-Key must contain 1 to 128 letters, digits, dots, underscores, colons, or hyphens",
        ))
    }
}

fn canonical_json_string(value: &serde_json::Value) -> String {
    match value {
        serde_json::Value::Null => "null".to_owned(),
        serde_json::Value::Bool(value) => value.to_string(),
        serde_json::Value::Number(value) => value.to_string(),
        serde_json::Value::String(value) => {
            serde_json::to_string(value).unwrap_or_else(|_| "\"\"".to_owned())
        }
        serde_json::Value::Array(values) => {
            let items = values
                .iter()
                .map(canonical_json_string)
                .collect::<Vec<_>>()
                .join(",");
            format!("[{items}]")
        }
        serde_json::Value::Object(values) => {
            let mut keys = values.keys().collect::<Vec<_>>();
            keys.sort_unstable();
            let items = keys
                .into_iter()
                .filter(|key| !values[*key].is_null())
                .map(|key| {
                    format!(
                        "{}:{}",
                        serde_json::to_string(key).unwrap_or_else(|_| "\"\"".to_owned()),
                        canonical_json_string(&values[key])
                    )
                })
                .collect::<Vec<_>>()
                .join(",");
            format!("{{{items}}}")
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_required_headers_and_derives_request_no() {
        let parsed =
            parse_sdkwork_write_command_headers(Some(" idem-1 "), Some("hash-1"), None, |key| {
                format!("request-{key}")
            })
            .expect("headers");
        assert_eq!(parsed.idempotency_key, "idem-1");
        assert_eq!(parsed.request_hash, "hash-1");
        assert_eq!(parsed.request_no, "request-idem-1");
    }

    #[test]
    fn missing_or_blank_required_headers_fail_closed() {
        for (key, hash) in [(None, Some("h")), (Some(""), Some("h")), (Some("k"), None)] {
            let error = parse_sdkwork_write_command_headers(key, hash, None, |_| "r".to_owned())
                .expect_err("required header");
            assert!(matches!(
                error,
                SdkWorkWriteCommandHeaderError::MissingHeader(_)
            ));
        }
    }

    #[test]
    fn idempotency_key_enforces_openapi_schema() {
        let too_long = "a".repeat(IDEMPOTENCY_KEY_MAX_LEN + 1);
        for bad in ["key with space", "key/slash", too_long.as_str()] {
            let error =
                parse_sdkwork_write_command_headers(Some(bad), Some("h"), None, |_| "r".to_owned())
                    .expect_err("invalid key");
            assert!(matches!(
                error,
                SdkWorkWriteCommandHeaderError::InvalidHeader(_)
            ));
        }
        let parsed =
            parse_sdkwork_write_command_headers(Some("k:1_2-3.x"), Some("h"), None, |_| {
                "r".to_owned()
            })
            .expect("charset-valid key");
        assert_eq!(parsed.idempotency_key, "k:1_2-3.x");
    }

    #[test]
    fn canonical_hash_is_deterministic_and_key_order_insensitive() {
        let first = sdkwork_stable_canonical_json_request_hash(
            "scope",
            &serde_json::json!({"b": 1, "a": "x"}),
        );
        let second = sdkwork_stable_canonical_json_request_hash(
            "scope",
            &serde_json::json!({"a": "x", "b": 1}),
        );
        assert_eq!(first, second);
        assert!(!first.is_empty());
    }

    #[test]
    fn route_param_payload_is_injected() {
        let payload = sdkwork_write_payload_with_route_param(
            "paymentId",
            "p-1",
            &serde_json::json!({"reason": "duplicate"}),
        );
        assert_eq!(payload["paymentId"], "p-1");
        assert_eq!(payload["reason"], "duplicate");
    }
}
