//! SDKWork HTTP API wire contracts (`API_SPEC.md` §14–§16).

use std::fmt;

use serde::de::{self, Unexpected, Visitor};
use serde::{Deserialize, Deserializer, Serialize};

/// Response header echoing `SdkWorkApiResponse.traceId` / `ProblemDetail.traceId`.
pub const SDKWORK_TRACE_ID_HEADER: &str = "X-SdkWork-Trace-Id";

/// Canonical success result code for HTTP 2xx JSON bodies.
pub const SDKWORK_SUCCESS_CODE: i32 = 0;

/// Platform result codes (`API_SPEC.md` §15.3).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(i32)]
pub enum SdkWorkResultCode {
    Ok = 0,
    ValidationError = 40001,
    MalformedRequest = 40002,
    InvalidParameter = 40003,
    MissingRequiredField = 40004,
    AuthenticationRequired = 40101,
    TokenExpired = 40102,
    InvalidToken = 40103,
    SessionRevoked = 40104,
    PermissionRequired = 40301,
    InsufficientScope = 40302,
    TenantAccessDenied = 40303,
    OrganizationAccessDenied = 40304,
    NotFound = 40401,
    MethodNotAllowed = 40501,
    RequestTimeout = 40801,
    Conflict = 40901,
    Gone = 41001,
    /// A PSP (payment service provider) rejected the checkout/refund request
    /// (e.g. WeChat `SIGN_ERROR`, Stripe `Invalid API Key`). Deliberately in
    /// the 41xxx range — far from the 401xx authentication codes — so admin
    /// surfaces never mistake a payment-gateway rejection for a session/login
    /// problem.
    PaymentGatewayRejected = 41101,
    PreconditionFailed = 41201,
    PayloadTooLarge = 41301,
    UnsupportedMediaType = 41501,
    UnprocessableEntity = 42201,
    Locked = 42301,
    PreconditionRequired = 42801,
    RateLimitExceeded = 42901,
    QuotaExceeded = 60002,
    InternalError = 50001,
    BadGateway = 50201,
    ServiceUnavailable = 50301,
    GatewayTimeout = 50401,
}

impl SdkWorkResultCode {
    pub const fn as_i32(self) -> i32 {
        self as i32
    }

    pub const fn symbol(self) -> &'static str {
        match self {
            Self::Ok => "OK",
            Self::ValidationError => "VALIDATION_ERROR",
            Self::MalformedRequest => "MALFORMED_REQUEST",
            Self::InvalidParameter => "INVALID_PARAMETER",
            Self::MissingRequiredField => "MISSING_REQUIRED_FIELD",
            Self::AuthenticationRequired => "AUTHENTICATION_REQUIRED",
            Self::TokenExpired => "TOKEN_EXPIRED",
            Self::InvalidToken => "INVALID_TOKEN",
            Self::SessionRevoked => "SESSION_REVOKED",
            Self::PermissionRequired => "PERMISSION_REQUIRED",
            Self::InsufficientScope => "INSUFFICIENT_SCOPE",
            Self::TenantAccessDenied => "TENANT_ACCESS_DENIED",
            Self::OrganizationAccessDenied => "ORGANIZATION_ACCESS_DENIED",
            Self::NotFound => "NOT_FOUND",
            Self::MethodNotAllowed => "METHOD_NOT_ALLOWED",
            Self::RequestTimeout => "REQUEST_TIMEOUT",
            Self::Conflict => "CONFLICT",
            Self::Gone => "GONE",
            Self::PaymentGatewayRejected => "PAYMENT_GATEWAY_REJECTED",
            Self::PreconditionFailed => "PRECONDITION_FAILED",
            Self::PayloadTooLarge => "PAYLOAD_TOO_LARGE",
            Self::UnsupportedMediaType => "UNSUPPORTED_MEDIA_TYPE",
            Self::UnprocessableEntity => "UNPROCESSABLE_ENTITY",
            Self::Locked => "LOCKED",
            Self::PreconditionRequired => "PRECONDITION_REQUIRED",
            Self::RateLimitExceeded => "RATE_LIMIT_EXCEEDED",
            Self::QuotaExceeded => "QUOTA_EXCEEDED",
            Self::InternalError => "INTERNAL_ERROR",
            Self::BadGateway => "BAD_GATEWAY",
            Self::ServiceUnavailable => "SERVICE_UNAVAILABLE",
            Self::GatewayTimeout => "GATEWAY_TIMEOUT",
        }
    }

    pub const fn http_status_code(self) -> u16 {
        match self {
            Self::Ok => 200,
            Self::ValidationError
            | Self::MalformedRequest
            | Self::InvalidParameter
            | Self::MissingRequiredField => 400,
            Self::AuthenticationRequired
            | Self::TokenExpired
            | Self::InvalidToken
            | Self::SessionRevoked => 401,
            Self::PermissionRequired
            | Self::InsufficientScope
            | Self::TenantAccessDenied
            | Self::OrganizationAccessDenied => 403,
            Self::NotFound => 404,
            Self::MethodNotAllowed => 405,
            Self::RequestTimeout => 408,
            Self::Conflict => 409,
            Self::Gone => 410,
            Self::PaymentGatewayRejected => 400,
            Self::PreconditionFailed => 412,
            Self::PayloadTooLarge => 413,
            Self::UnsupportedMediaType => 415,
            Self::UnprocessableEntity => 422,
            Self::Locked => 423,
            Self::PreconditionRequired => 428,
            Self::RateLimitExceeded | Self::QuotaExceeded => 429,
            Self::InternalError => 500,
            Self::BadGateway => 502,
            Self::ServiceUnavailable => 503,
            Self::GatewayTimeout => 504,
        }
    }

    pub const fn title(self) -> &'static str {
        match self {
            Self::Ok => "OK",
            Self::ValidationError => "Validation failed",
            Self::MalformedRequest => "Malformed request",
            Self::InvalidParameter => "Invalid parameter",
            Self::MissingRequiredField => "Missing required field",
            Self::AuthenticationRequired => "Authentication required",
            Self::TokenExpired => "Token expired",
            Self::InvalidToken => "Invalid token",
            Self::SessionRevoked => "Session revoked",
            Self::PermissionRequired => "Permission required",
            Self::InsufficientScope => "Insufficient scope",
            Self::TenantAccessDenied => "Tenant access denied",
            Self::OrganizationAccessDenied => "Organization access denied",
            Self::NotFound => "Not found",
            Self::MethodNotAllowed => "Method not allowed",
            Self::RequestTimeout => "Request timeout",
            Self::Conflict => "Conflict",
            Self::Gone => "Gone",
            Self::PaymentGatewayRejected => "Payment gateway rejected the request",
            Self::PreconditionFailed => "Precondition failed",
            Self::PayloadTooLarge => "Payload too large",
            Self::UnsupportedMediaType => "Unsupported media type",
            Self::UnprocessableEntity => "Unprocessable entity",
            Self::Locked => "Locked",
            Self::PreconditionRequired => "Precondition required",
            Self::RateLimitExceeded => "Rate limit exceeded",
            Self::QuotaExceeded => "Quota exceeded",
            Self::InternalError => "Internal server error",
            Self::BadGateway => "Bad gateway",
            Self::ServiceUnavailable => "Service unavailable",
            Self::GatewayTimeout => "Gateway timeout",
        }
    }
}

/// Canonical HTTP success envelope (`API_SPEC.md` §15.1.1).
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SdkWorkApiResponse<T> {
    pub code: i32,
    pub data: T,
    pub trace_id: String,
}

impl<T> SdkWorkApiResponse<T> {
    pub fn success(data: T, trace_id: impl Into<String>) -> Self {
        Self {
            code: SDKWORK_SUCCESS_CODE,
            data,
            trace_id: trace_id.into(),
        }
    }
}

/// Pagination mode (`API_SPEC.md` §16).
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PageMode {
    Offset,
    Cursor,
}

/// Standard list pagination metadata (`API_SPEC.md` §16).
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PageInfo {
    pub mode: PageMode,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub page: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub page_size: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub total_items: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub total_pages: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub next_cursor: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub has_more: Option<bool>,
}

/// Standard list payload inside `SdkWorkApiResponse.data`.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SdkWorkPageData<T> {
    pub items: Vec<T>,
    pub page_info: PageInfo,
}

/// SQL window column alias for `COUNT(*) OVER()` total row counts in list queries.
pub const LIST_TOTAL_SQL_COLUMN: &str = "__list_total";

/// Default page size for offset list queries (`SdkWorkListQuery.pageSize`).
pub const DEFAULT_LIST_PAGE_SIZE: i32 = 20;

/// Maximum allowed page size for offset list queries (`SdkWorkListQuery.pageSize`).
pub const MAX_LIST_PAGE_SIZE: i32 = 200;

/// Maximum allowed page number for offset list queries.
///
/// Offset pagination targets low-volume stable lists (PAGINATION_SPEC §3);
/// deep pages degrade to O(offset) scans and can overflow `(page - 1) * page_size`
/// on i64. 10_000 pages × 200 rows caps the practical offset range at 2M rows.
pub const MAX_LIST_PAGE: i64 = 10_000;

/// Parsed offset pagination parameters for database-backed list handlers.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct OffsetListPageParams {
    pub page: i64,
    pub page_size: i64,
    pub offset: i64,
}

impl OffsetListPageParams {
    pub fn parse(page: Option<i64>, page_size: Option<i64>) -> Self {
        let page_size = page_size
            .unwrap_or(i64::from(DEFAULT_LIST_PAGE_SIZE))
            .clamp(1, i64::from(MAX_LIST_PAGE_SIZE));
        let page = page.unwrap_or(1).clamp(1, MAX_LIST_PAGE);
        let offset = (page - 1) * page_size;
        Self {
            page,
            page_size,
            offset,
        }
    }
}

/// Validates standard offset list params per PAGINATION_SPEC; rejects out-of-range values instead of clamping.
pub fn validated_offset_list_params(
    page: Option<i64>,
    page_size: Option<i64>,
) -> Result<OffsetListPageParams, SdkWorkResultCode> {
    let page_size = page_size.unwrap_or(i64::from(DEFAULT_LIST_PAGE_SIZE));
    if page_size < 1 || page_size > i64::from(MAX_LIST_PAGE_SIZE) {
        return Err(SdkWorkResultCode::InvalidParameter);
    }
    let page = page.unwrap_or(1);
    if page < 1 || page > MAX_LIST_PAGE {
        return Err(SdkWorkResultCode::InvalidParameter);
    }
    Ok(OffsetListPageParams {
        page,
        page_size,
        offset: (page - 1) * page_size,
    })
}

/// Build offset pagination metadata from already-validated `page` and `page_size` values.
///
/// Callers must pre-validate through [`validated_offset_list_params`]; the
/// offset arithmetic is checked so a misuse cannot overflow i64.
pub fn offset_list_page_params_from_values(page: i64, page_size: i64) -> OffsetListPageParams {
    OffsetListPageParams {
        page,
        page_size,
        offset: (page - 1).saturating_mul(page_size),
    }
}

/// Validates cursor list params per `PAGINATION_SPEC`; rejects out-of-range `page_size` instead of clamping.
pub fn validated_cursor_list_params(
    page_size: Option<i32>,
    cursor: Option<&str>,
) -> Result<CursorListPageParams, SdkWorkResultCode> {
    let page_size = page_size.unwrap_or(DEFAULT_LIST_PAGE_SIZE);
    if page_size < 1 || page_size > MAX_LIST_PAGE_SIZE {
        return Err(SdkWorkResultCode::InvalidParameter);
    }
    let offset = parse_offset_list_cursor(cursor)?;
    Ok(CursorListPageParams {
        page_size: page_size as usize,
        offset,
    })
}

/// Resolved list pagination mode for SQL-backed handlers.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ResolvedListPageParams {
    Offset(OffsetListPageParams),
    Cursor(CursorListPageParams),
}

fn query_has_page_key(query: &std::collections::HashMap<String, String>) -> bool {
    query
        .get("page")
        .map(|value| !value.trim().is_empty())
        .unwrap_or(false)
}

fn query_has_cursor_key(query: &std::collections::HashMap<String, String>) -> bool {
    query
        .get("cursor")
        .map(|value| !value.trim().is_empty())
        .unwrap_or(false)
}

fn parse_query_page_size_i32(query: &std::collections::HashMap<String, String>) -> Option<i32> {
    query
        .get("page_size")
        .and_then(|value| value.parse::<i32>().ok())
}

fn parse_query_page_i64(query: &std::collections::HashMap<String, String>) -> Option<i64> {
    query
        .get("page")
        .and_then(|value| value.parse::<i64>().ok())
}

/// Validates standard list query params; rejects `page` + `cursor` together and out-of-range values.
pub fn validated_list_page_params_from_map(
    query: &std::collections::HashMap<String, String>,
) -> Result<ResolvedListPageParams, SdkWorkResultCode> {
    let has_page = query_has_page_key(query);
    let has_cursor = query_has_cursor_key(query);
    if has_page && has_cursor {
        return Err(SdkWorkResultCode::InvalidParameter);
    }

    if has_cursor {
        return Ok(ResolvedListPageParams::Cursor(
            validated_cursor_list_params(
                parse_query_page_size_i32(query),
                query.get("cursor").map(String::as_str),
            )?,
        ));
    }

    Ok(ResolvedListPageParams::Offset(
        validated_offset_list_params(
            parse_query_page_i64(query),
            parse_query_page_size_i32(query).map(i64::from),
        )?,
    ))
}

/// Parse standard list query keys: `page` and `page_size`.
pub fn offset_list_page_params_from_map(
    query: &std::collections::HashMap<String, String>,
) -> OffsetListPageParams {
    let page = query
        .get("page")
        .and_then(|value| value.parse::<i64>().ok());
    let page_size = query
        .get("page_size")
        .and_then(|value| value.parse::<i64>().ok());
    OffsetListPageParams::parse(page, page_size)
}

/// Build offset-mode `PageInfo` with total counts for SQL-backed list responses.
pub fn offset_list_page_info(total_items: i64, params: OffsetListPageParams) -> PageInfo {
    let total_pages = if total_items == 0 {
        0
    } else {
        ((total_items + params.page_size - 1) / params.page_size) as i32
    };
    let has_more = params.page * params.page_size < total_items;
    PageInfo {
        mode: PageMode::Offset,
        page: Some(params.page as i32),
        page_size: Some(params.page_size as i32),
        total_items: Some(total_items.to_string()),
        total_pages: Some(total_pages),
        next_cursor: None,
        has_more: Some(has_more),
    }
}

/// Build standard `SdkWorkPageData` for typed list handlers.
pub fn offset_list_page_data<T>(
    items: Vec<T>,
    total_items: i64,
    params: OffsetListPageParams,
) -> SdkWorkPageData<T> {
    SdkWorkPageData {
        items,
        page_info: offset_list_page_info(total_items, params),
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct OffsetLimitPage<T> {
    pub items: Vec<T>,
    pub next_cursor: Option<String>,
    pub has_more: bool,
}

/// Parse an offset list cursor token. Missing or blank cursor resolves to `0`.
pub fn parse_offset_list_cursor(cursor: Option<&str>) -> Result<usize, SdkWorkResultCode> {
    let Some(cursor) = cursor.map(str::trim).filter(|value| !value.is_empty()) else {
        return Ok(0);
    };
    cursor
        .parse::<usize>()
        .map_err(|_| SdkWorkResultCode::InvalidParameter)
}

/// Collect at most `limit + 1` items from an ordered iterator after skipping `offset` rows.
pub fn offset_limit_page_from_iter<I, T>(iter: I, limit: usize, offset: usize) -> OffsetLimitPage<T>
where
    I: Iterator<Item = T>,
{
    if limit == 0 {
        return OffsetLimitPage {
            items: Vec::new(),
            next_cursor: None,
            has_more: false,
        };
    }

    let mut skipped = 0usize;
    let mut items = Vec::with_capacity(limit.saturating_add(1));
    let mut has_more = false;

    for item in iter {
        if skipped < offset {
            skipped += 1;
            continue;
        }
        items.push(item);
        if items.len() > limit {
            has_more = true;
            break;
        }
    }

    if has_more {
        items.truncate(limit);
    }

    let next_cursor = has_more.then(|| offset.saturating_add(items.len()).to_string());
    OffsetLimitPage {
        items,
        next_cursor,
        has_more,
    }
}

/// Parsed offset-mode cursor list parameters (`page_size` + numeric offset cursor).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CursorListPageParams {
    pub page_size: usize,
    pub offset: usize,
}

impl CursorListPageParams {
    pub fn resolve(
        page_size: Option<i32>,
        cursor: Option<&str>,
    ) -> Result<Self, SdkWorkResultCode> {
        let page_size = page_size
            .map(i64::from)
            .unwrap_or(i64::from(DEFAULT_LIST_PAGE_SIZE))
            .clamp(1, i64::from(MAX_LIST_PAGE_SIZE)) as usize;
        let offset = parse_offset_list_cursor(cursor)?;
        Ok(Self { page_size, offset })
    }
}

fn deserialize_option_query_u64<'de, D>(deserializer: D) -> Result<Option<u64>, D::Error>
where
    D: Deserializer<'de>,
{
    struct OptU64Visitor;

    impl Visitor<'_> for OptU64Visitor {
        type Value = Option<u64>;

        fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
            formatter.write_str("an optional unsigned integer")
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
            u64::try_from(value)
                .map(Some)
                .map_err(|_| E::invalid_value(Unexpected::Signed(value), &self))
        }

        fn visit_u64<E>(self, value: u64) -> Result<Self::Value, E>
        where
            E: de::Error,
        {
            Ok(Some(value))
        }

        fn visit_str<E>(self, value: &str) -> Result<Self::Value, E>
        where
            E: de::Error,
        {
            let trimmed = value.trim();
            if trimmed.is_empty() {
                return Ok(None);
            }
            trimmed.parse::<u64>().map(Some).map_err(E::custom)
        }
    }

    deserializer.deserialize_any(OptU64Visitor)
}

pub fn deserialize_option_query_i32<'de, D>(deserializer: D) -> Result<Option<i32>, D::Error>
where
    D: Deserializer<'de>,
{
    struct OptI32Visitor;

    impl Visitor<'_> for OptI32Visitor {
        type Value = Option<i32>;

        fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
            formatter.write_str("an optional integer")
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
            i32::try_from(value)
                .map(Some)
                .map_err(|_| E::invalid_value(Unexpected::Signed(value), &self))
        }

        fn visit_u64<E>(self, value: u64) -> Result<Self::Value, E>
        where
            E: de::Error,
        {
            i32::try_from(value)
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
            trimmed.parse::<i32>().map(Some).map_err(E::custom)
        }
    }

    deserializer.deserialize_any(OptI32Visitor)
}

/// Deserialize optional HTTP query strings; empty or whitespace-only values become `None`.
pub fn deserialize_option_query_string<'de, D>(deserializer: D) -> Result<Option<String>, D::Error>
where
    D: Deserializer<'de>,
{
    struct OptStringVisitor;

    impl Visitor<'_> for OptStringVisitor {
        type Value = Option<String>;

        fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
            formatter.write_str("an optional string query parameter")
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

        fn visit_str<E>(self, value: &str) -> Result<Self::Value, E>
        where
            E: de::Error,
        {
            let trimmed = value.trim();
            if trimmed.is_empty() {
                Ok(None)
            } else {
                Ok(Some(trimmed.to_owned()))
            }
        }

        fn visit_string<E>(self, value: String) -> Result<Self::Value, E>
        where
            E: de::Error,
        {
            self.visit_str(&value)
        }
    }

    deserializer.deserialize_any(OptStringVisitor)
}

/// Standard cursor/offset list query (`page_size` HTTP query wire).
#[derive(Clone, Debug, Default, Eq, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct SdkWorkCursorListQuery {
    #[serde(
        rename = "page_size",
        default,
        deserialize_with = "deserialize_option_query_i32"
    )]
    pub page_size: Option<i32>,
    pub cursor: Option<String>,
}

impl SdkWorkCursorListQuery {
    pub fn resolve(&self) -> Result<CursorListPageParams, SdkWorkResultCode> {
        let page_size = self.page_size.unwrap_or(DEFAULT_LIST_PAGE_SIZE);
        if page_size < 1 || page_size > MAX_LIST_PAGE_SIZE {
            return Err(SdkWorkResultCode::InvalidParameter);
        }
        let offset = parse_offset_list_cursor(self.cursor.as_deref())?;
        Ok(CursorListPageParams {
            page_size: page_size as usize,
            offset,
        })
    }

    /// Resolve only the strictly-validated page size without interpreting the cursor.
    ///
    /// Keyset-paged list handlers carry opaque (signed/encrypted) cursors that
    /// must be decoded by the operation's own cursor codec, never parsed as a
    /// numeric offset. Numeric offset cursors are forbidden by
    /// `PAGINATION_SPEC.md` (§2.4, §12); this returns the validated
    /// `page_size` (1..=MAX_LIST_PAGE_SIZE) or `InvalidParameter` (40003).
    pub fn resolve_page_size(&self) -> Result<usize, SdkWorkResultCode> {
        let page_size = self.page_size.unwrap_or(DEFAULT_LIST_PAGE_SIZE);
        if page_size < 1 || page_size > MAX_LIST_PAGE_SIZE {
            return Err(SdkWorkResultCode::InvalidParameter);
        }
        Ok(page_size as usize)
    }
}

/// Single-field page size query (`page_size` HTTP query wire).
#[derive(Clone, Debug, Default, Eq, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct SdkWorkPageSizeQuery {
    #[serde(
        rename = "page_size",
        default,
        deserialize_with = "deserialize_option_query_i32"
    )]
    pub page_size: Option<i32>,
}

impl SdkWorkPageSizeQuery {
    pub fn resolve(&self) -> usize {
        self.page_size
            .map(i64::from)
            .unwrap_or(i64::from(DEFAULT_LIST_PAGE_SIZE))
            .clamp(1, i64::from(MAX_LIST_PAGE_SIZE)) as usize
    }

    pub fn resolve_i64(&self) -> i64 {
        i64::try_from(self.resolve()).unwrap_or(i64::from(MAX_LIST_PAGE_SIZE))
    }
}

/// Sequence-window list query for message/timeline feeds (`afterSeq` + `page_size`).
#[derive(Clone, Debug, Default, Eq, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct SdkWorkSeqWindowQuery {
    #[serde(default, deserialize_with = "deserialize_option_query_u64")]
    pub after_seq: Option<u64>,
    #[serde(
        rename = "page_size",
        default,
        deserialize_with = "deserialize_option_query_i32"
    )]
    pub page_size: Option<i32>,
}

impl SdkWorkSeqWindowQuery {
    pub fn resolved_page_size(&self) -> usize {
        self.page_size
            .map(i64::from)
            .unwrap_or(i64::from(DEFAULT_LIST_PAGE_SIZE))
            .clamp(1, i64::from(MAX_LIST_PAGE_SIZE)) as usize
    }
}

/// Build standard offset-mode `PageInfo` for numeric cursor windows.
pub fn offset_limit_page_info(next_cursor: Option<String>, has_more: bool) -> PageInfo {
    offset_window_page_info(None, next_cursor, has_more)
}

/// Build offset-mode `PageInfo` for SQL-backed `limit + 1` list handlers without a total count.
pub fn offset_paged_list_page_info(params: OffsetListPageParams, has_more: bool) -> PageInfo {
    PageInfo {
        mode: PageMode::Offset,
        page: Some(params.page as i32),
        page_size: Some(params.page_size as i32),
        total_items: None,
        total_pages: None,
        next_cursor: has_more.then(|| (params.offset + params.page_size).to_string()),
        has_more: Some(has_more),
    }
}

/// Build offset-mode `PageInfo` including resolved `pageSize` when available.
pub fn offset_window_page_info(
    page_size: Option<usize>,
    next_cursor: Option<String>,
    has_more: bool,
) -> PageInfo {
    PageInfo {
        mode: PageMode::Offset,
        page: None,
        page_size: page_size.map(|value| value as i32),
        total_items: None,
        total_pages: None,
        next_cursor,
        has_more: Some(has_more),
    }
}

/// Build cursor-mode `PageInfo` for opaque or numeric continuation tokens.
pub fn cursor_window_page_info(
    page_size: Option<usize>,
    next_cursor: Option<String>,
    has_more: bool,
) -> PageInfo {
    PageInfo {
        mode: PageMode::Cursor,
        page: None,
        page_size: page_size.map(|value| value as i32),
        total_items: None,
        total_pages: None,
        next_cursor,
        has_more: Some(has_more),
    }
}

/// Build standard cursor-mode `SdkWorkPageData`.
pub fn cursor_list_page_data<T>(
    items: Vec<T>,
    page_size: usize,
    next_cursor: Option<String>,
    has_more: bool,
) -> SdkWorkPageData<T> {
    SdkWorkPageData {
        items,
        page_info: cursor_window_page_info(Some(page_size), next_cursor, has_more),
    }
}

/// Standard single-resource payload inside `SdkWorkApiResponse.data`.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct SdkWorkResourceData<T> {
    pub item: T,
}

/// Serialize standard single-resource payload (`SdkWorkResourceResponse.data`).
pub fn sdkwork_resource_json(item: serde_json::Value) -> serde_json::Value {
    serde_json::to_value(SdkWorkResourceData { item })
        .unwrap_or_else(|_| serde_json::json!({ "item": serde_json::Value::Null }))
}

/// Serialize hierarchical tree payload: `{ "item": { "nodes": [...] } }`.
pub fn sdkwork_tree_resource_json(nodes: Vec<serde_json::Value>) -> serde_json::Value {
    sdkwork_resource_json(serde_json::json!({ "nodes": nodes }))
}

/// Standard command payload inside `SdkWorkApiResponse.data`.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SdkWorkCommandData {
    pub accepted: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub resource_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub status: Option<String>,
}

impl SdkWorkCommandData {
    pub fn accepted() -> Self {
        Self {
            accepted: true,
            resource_id: None,
            status: None,
        }
    }
}

/// Request routing context attached to `ProblemDetail` (`API_SPEC.md` §15.2).
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct SdkWorkProblemRouting {
    pub method: Option<String>,
    pub route_template: Option<String>,
    pub fallback_path: Option<String>,
    pub operation_id: Option<String>,
}

impl SdkWorkProblemRouting {
    pub fn from_parts(
        method: Option<&str>,
        route_template: Option<&str>,
        fallback_path: Option<&str>,
        operation_id: Option<&str>,
    ) -> Self {
        Self {
            method: non_empty_text(method),
            route_template: non_empty_text(route_template),
            fallback_path: non_empty_text(fallback_path),
            operation_id: non_empty_text(operation_id),
        }
    }

    /// RFC 9457 `instance`: `{METHOD} {routeTemplate}` with safe fallback redaction.
    pub fn instance(&self) -> Option<String> {
        let route = self
            .route_template
            .as_deref()
            .or(self.fallback_path.as_deref())?;
        let route = if self.route_template.is_some() {
            route.to_owned()
        } else {
            redact_http_path_segments(route)
        };
        let method = self
            .method
            .as_deref()
            .unwrap_or("GET")
            .trim()
            .to_ascii_uppercase();
        Some(format!("{method} {route}"))
    }
}

/// Redact identifier-like HTTP path segments for Problem `instance` values
/// (`API_SPEC.md` §15.2).
///
/// Recognizes numeric IDs, UUID-like values, business resource IDs following
/// the `<prefix>_<suffix>` pattern (suffix ≥8 alphanumeric chars, e.g.
/// `c_direct_09a8255a1fd3632675c2d355`), and long opaque tokens (≥16
/// alphanumeric chars). Route template segments (e.g. `{conversationId}`,
/// `{*path}`) and known API path literals (e.g. `im`, `v3`, `api`, `chat`,
/// `conversations`, `messages`) are preserved because they do not match these
/// identifier patterns.
pub fn redact_http_path_segments(path: &str) -> String {
    path.split('/')
        .map(|segment| {
            if segment.is_empty() {
                return String::new();
            }
            if is_redactable_id_segment(segment) {
                "{id}".to_owned()
            } else {
                segment.to_owned()
            }
        })
        .collect::<Vec<_>>()
        .join("/")
}

/// Checks whether an HTTP path segment looks like a redactable resource identifier.
fn is_redactable_id_segment(segment: &str) -> bool {
    // Pure numeric IDs (e.g. `42`, `99`).
    if segment.chars().all(|ch| ch.is_ascii_digit()) {
        return true;
    }
    // UUID-like IDs (≥32 hex digits with optional dashes).
    if segment.len() >= 32
        && segment
            .chars()
            .all(|ch| ch.is_ascii_hexdigit() || ch == '-')
    {
        return true;
    }
    // Business resource IDs: `<prefix>_<suffix>` where the final suffix is
    // ≥8 alphanumeric chars (e.g. `c_direct_09a8255a1fd3632675c2d355`).
    if let Some((_, suffix)) = segment.rsplit_once('_') {
        if suffix.len() >= 8 && suffix.chars().all(|ch| ch.is_ascii_alphanumeric()) {
            return true;
        }
    }
    // Long opaque tokens (≥16 alphanumeric chars, no separators).
    if segment.len() >= 16 && segment.chars().all(|ch| ch.is_ascii_alphanumeric()) {
        return true;
    }
    false
}

fn non_empty_text(value: Option<&str>) -> Option<String> {
    value
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_owned)
}

/// RFC 6749 OAuth 2.0 error response body for external protocol endpoints (`API_SPEC.md` §4.5.2).
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct OAuthProtocolErrorBody {
    pub error: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error_description: Option<String>,
}

impl OAuthProtocolErrorBody {
    pub fn new(error: impl Into<String>, description: impl Into<String>) -> Self {
        let description = description.into();
        Self {
            error: error.into(),
            error_description: if description.trim().is_empty() {
                None
            } else {
                Some(description)
            },
        }
    }

    /// Maps IAM internal OAuth error codes to RFC 6749 `error` values.
    pub fn from_iam_oauth_code(code: &str, message: &str) -> Self {
        let oauth_error = if code.contains("invalid_client") || code.contains("client_invalid") {
            "invalid_client"
        } else if code.contains("invalid_grant")
            || code.contains("token_exchange")
            || code.contains("refresh_token")
            || code.contains("revoke_failed")
        {
            "invalid_grant"
        } else if code.contains("access_denied") || code.contains("denied") {
            "access_denied"
        } else if code.contains("unsupported") {
            "unsupported_grant_type"
        } else if code.contains("scope") {
            "invalid_scope"
        } else if code.contains("rate") || code.contains("rate_limited") {
            "temporarily_unavailable"
        } else if code.contains("unavailable") || code.contains("failed") {
            "server_error"
        } else if code.contains("unauthorized") {
            "unauthorized_client"
        } else {
            "invalid_request"
        };
        Self::new(oauth_error, message)
    }
}

/// RFC 9457 `application/problem+json` body (`API_SPEC.md` §15.2).
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SdkWorkProblemDetail {
    #[serde(rename = "type")]
    pub problem_type: String,
    pub title: String,
    pub status: u16,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub instance: Option<String>,
    pub code: i32,
    pub trace_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub operation_id: Option<String>,
    /// i18n message key derived from `code` (`API_SPEC.md` §15.2).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub i18n_key: Option<String>,
}

impl SdkWorkProblemDetail {
    pub fn platform(
        result_code: SdkWorkResultCode,
        detail: impl Into<String>,
        trace_id: impl Into<String>,
    ) -> Self {
        Self::platform_body(result_code, detail, trace_id)
    }

    pub fn platform_enriched(
        result_code: SdkWorkResultCode,
        detail: impl Into<String>,
        trace_id: impl Into<String>,
        routing: SdkWorkProblemRouting,
    ) -> Self {
        Self::platform_body(result_code, detail, trace_id).with_routing(routing)
    }

    pub fn with_routing(mut self, routing: SdkWorkProblemRouting) -> Self {
        self.instance = routing.instance();
        self.operation_id = routing.operation_id;
        self
    }

    /// Client-safe Problem `detail` — internal failures must not leak implementation details.
    pub fn client_safe_detail(result_code: SdkWorkResultCode, detail: &str) -> String {
        match result_code {
            SdkWorkResultCode::InternalError => "An internal error occurred".to_owned(),
            SdkWorkResultCode::ServiceUnavailable => {
                "A required dependency is temporarily unavailable".to_owned()
            }
            _ if detail.trim().is_empty() => result_code.title().to_owned(),
            _ => detail.to_owned(),
        }
    }

    fn platform_body(
        result_code: SdkWorkResultCode,
        detail: impl Into<String>,
        trace_id: impl Into<String>,
    ) -> Self {
        let detail_text = Self::client_safe_detail(result_code, &detail.into());
        // i18n key auto-filled per API_SPEC.md §15.2 (`errors.result.<code>`).
        let i18n_key = Some(format!("errors.result.{}", result_code.as_i32()));
        Self {
            problem_type: format!("https://docs.sdkwork.com/problems/{}", result_code.as_i32()),
            title: result_code.title().to_string(),
            status: result_code.http_status_code(),
            detail: if detail_text.is_empty() {
                None
            } else {
                Some(detail_text)
            },
            instance: None,
            code: result_code.as_i32(),
            trace_id: trace_id.into(),
            operation_id: None,
            i18n_key,
        }
    }
}

/// Maps legacy Cloud Router string wire codes and symbolic aliases to platform codes.
///
/// **Note:** This mapper exists for sibling SDKWork applications that still emit
/// legacy wire codes. SDKWork IM (pre-launch) does not use or invoke this mapper.
pub fn legacy_wire_result_code(wire_code: &str) -> SdkWorkResultCode {
    match wire_code.trim() {
        "2000" => SdkWorkResultCode::Ok,
        "4001" => SdkWorkResultCode::ValidationError,
        "4004" => SdkWorkResultCode::NotFound,
        "4010" => SdkWorkResultCode::AuthenticationRequired,
        "4040" | "not_found" => SdkWorkResultCode::NotFound,
        "4090" | "conflict" => SdkWorkResultCode::Conflict,
        "4220" => SdkWorkResultCode::UnprocessableEntity,
        "5000" | "5001" | "4000" => SdkWorkResultCode::InternalError,
        "5030" => SdkWorkResultCode::ServiceUnavailable,
        "invalid_input" | "validation_error" => SdkWorkResultCode::ValidationError,
        "forbidden" => SdkWorkResultCode::PermissionRequired,
        "rate_limited" => SdkWorkResultCode::RateLimitExceeded,
        "provider_error" => SdkWorkResultCode::BadGateway,
        _ => SdkWorkResultCode::InternalError,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn success_envelope_uses_zero_code() {
        let body = SdkWorkApiResponse::success(SdkWorkResourceData { item: 42 }, "trace-1");
        assert_eq!(0, body.code);
        assert_eq!("trace-1", body.trace_id);
    }

    #[test]
    fn platform_codes_match_spec_ranges() {
        assert_eq!(40001, SdkWorkResultCode::ValidationError.as_i32());
        assert_eq!(40101, SdkWorkResultCode::AuthenticationRequired.as_i32());
        assert_eq!(50001, SdkWorkResultCode::InternalError.as_i32());
    }

    #[test]
    fn legacy_cloud_router_codes_map_to_platform_codes() {
        assert_eq!(40401, legacy_wire_result_code("4004").as_i32());
        assert_eq!(40101, legacy_wire_result_code("4010").as_i32());
        assert_eq!(50301, legacy_wire_result_code("5030").as_i32());
    }

    #[test]
    fn problem_detail_uses_numeric_code_and_trace_id() {
        let problem = SdkWorkProblemDetail::platform(
            SdkWorkResultCode::NotFound,
            "Workspace not found",
            "trace-404",
        );
        let json = serde_json::to_value(problem).expect("serialize problem");
        assert_eq!(json["code"], 40401);
        assert_eq!(json["status"], 404);
        assert_eq!(json["traceId"], "trace-404");
        assert_eq!(json["detail"], "Workspace not found");
    }

    #[test]
    fn problem_detail_enriched_with_instance_and_operation_id() {
        let routing = SdkWorkProblemRouting::from_parts(
            Some("get"),
            Some("/app/v3/api/wallet/transactions"),
            None,
            Some("wallet.transactions.list"),
        );
        let problem = SdkWorkProblemDetail::platform_enriched(
            SdkWorkResultCode::InternalError,
            "sql leak",
            "trace-500",
            routing,
        );
        let json = serde_json::to_value(problem).expect("serialize problem");
        assert_eq!(json["instance"], "GET /app/v3/api/wallet/transactions");
        assert_eq!(json["operationId"], "wallet.transactions.list");
        assert_eq!(json["detail"], "An internal error occurred");
    }

    #[test]
    fn problem_detail_includes_i18n_key() {
        let problem = SdkWorkProblemDetail::platform(
            SdkWorkResultCode::ServiceUnavailable,
            "dependency down",
            "trace-503",
        );
        assert_eq!(problem.i18n_key.as_deref(), Some("errors.result.50301"));
    }

    #[test]
    fn redact_http_path_segments_masks_ids() {
        assert_eq!(
            "/app/v3/api/users/{id}/orders/{id}",
            redact_http_path_segments("/app/v3/api/users/42/orders/99")
        );
        assert_eq!(
            "/im/v3/api/chat/conversations/{id}/messages",
            redact_http_path_segments(
                "/im/v3/api/chat/conversations/c_direct_09a8255a1fd3632675c2d355/messages"
            )
        );
    }

    #[test]
    fn validated_offset_list_params_rejects_invalid_page_size() {
        assert_eq!(
            validated_offset_list_params(Some(1), Some(0)),
            Err(SdkWorkResultCode::InvalidParameter)
        );
        assert_eq!(
            validated_offset_list_params(Some(1), Some(201)),
            Err(SdkWorkResultCode::InvalidParameter)
        );
    }

    #[test]
    fn validated_offset_list_params_rejects_invalid_page() {
        assert_eq!(
            validated_offset_list_params(Some(0), Some(20)),
            Err(SdkWorkResultCode::InvalidParameter)
        );
        assert_eq!(
            validated_offset_list_params(Some(MAX_LIST_PAGE + 1), Some(20)),
            Err(SdkWorkResultCode::InvalidParameter)
        );
        // A page large enough to overflow `(page - 1) * page_size` must be
        // rejected instead of wrapping.
        assert_eq!(
            validated_offset_list_params(Some(i64::MAX), Some(200)),
            Err(SdkWorkResultCode::InvalidParameter)
        );
    }

    #[test]
    fn offset_list_page_params_parse_clamps_excessive_page() {
        let params = OffsetListPageParams::parse(Some(i64::MAX), Some(200));
        assert_eq!(params.page, MAX_LIST_PAGE);
        assert_eq!(params.offset, (MAX_LIST_PAGE - 1) * 200);
    }

    #[test]
    fn validated_offset_list_params_defaults_match_spec() {
        let params = validated_offset_list_params(None, None).expect("defaults");
        assert_eq!(params.page, 1);
        assert_eq!(params.page_size, 20);
        assert_eq!(params.offset, 0);
    }

    #[test]
    fn validated_list_page_params_from_map_rejects_page_and_cursor_together() {
        let mut query = std::collections::HashMap::new();
        query.insert("page".to_owned(), "2".to_owned());
        query.insert("cursor".to_owned(), "20".to_owned());
        assert_eq!(
            validated_list_page_params_from_map(&query),
            Err(SdkWorkResultCode::InvalidParameter)
        );
    }

    #[test]
    fn validated_list_page_params_from_map_resolves_cursor_mode() {
        let mut query = std::collections::HashMap::new();
        query.insert("cursor".to_owned(), "40".to_owned());
        query.insert("page_size".to_owned(), "10".to_owned());
        let params = validated_list_page_params_from_map(&query).expect("cursor mode");
        assert_eq!(
            params,
            ResolvedListPageParams::Cursor(CursorListPageParams {
                page_size: 10,
                offset: 40,
            })
        );
    }

    #[test]
    fn validated_cursor_list_params_rejects_invalid_page_size() {
        assert_eq!(
            validated_cursor_list_params(Some(0), None),
            Err(SdkWorkResultCode::InvalidParameter)
        );
        assert_eq!(
            validated_cursor_list_params(Some(201), None),
            Err(SdkWorkResultCode::InvalidParameter)
        );
    }

    #[test]
    fn offset_list_page_params_default_to_spec_page_size() {
        let params = OffsetListPageParams::parse(None, None);
        assert_eq!(1, params.page);
        assert_eq!(20, params.page_size);
        assert_eq!(0, params.offset);
    }

    #[test]
    fn offset_paged_list_page_info_includes_page_and_page_size() {
        let params = OffsetListPageParams::parse(Some(2), Some(20));
        let info = offset_paged_list_page_info(params, true);
        assert_eq!(Some(PageMode::Offset), Some(info.mode));
        assert_eq!(Some(2), info.page);
        assert_eq!(Some(20), info.page_size);
        assert_eq!(Some(true), info.has_more);
        assert_eq!(Some("40".to_owned()), info.next_cursor);
    }

    #[test]
    fn offset_list_page_info_reports_has_more_from_total() {
        let params = OffsetListPageParams::parse(Some(1), Some(20));
        let info = offset_list_page_info(45, params);
        assert_eq!(Some(PageMode::Offset), Some(info.mode));
        assert_eq!(Some(3), info.total_pages);
        assert_eq!(Some(true), info.has_more);
        assert_eq!(Some("45".to_owned()), info.total_items);
    }

    #[test]
    fn offset_limit_page_from_iter_applies_cursor_without_materializing_full_collection() {
        let page = offset_limit_page_from_iter((1..=5).map(|value| value.to_string()), 2, 1);
        assert_eq!(page.items, vec!["2".to_owned(), "3".to_owned()]);
        assert!(page.has_more);
        assert_eq!(page.next_cursor.as_deref(), Some("3"));
    }

    #[test]
    fn parse_offset_list_cursor_defaults_to_zero() {
        assert_eq!(parse_offset_list_cursor(None).expect("missing cursor"), 0);
        assert_eq!(
            parse_offset_list_cursor(Some("  ")).expect("blank cursor"),
            0
        );
        assert_eq!(
            parse_offset_list_cursor(Some("4")).expect("numeric cursor"),
            4
        );
    }

    #[test]
    fn cursor_list_page_params_resolve_page_size() {
        let from_page_size =
            CursorListPageParams::resolve(Some(10), Some("20")).expect("page size");
        assert_eq!(from_page_size.page_size, 10);
        assert_eq!(from_page_size.offset, 20);
    }

    #[test]
    fn sdkwork_cursor_list_query_deserializes_page_size() {
        let from_page_size: SdkWorkCursorListQuery =
            serde_urlencoded::from_str("page_size=12&cursor=3").expect("page_size");
        assert_eq!(from_page_size.resolve().expect("resolve").page_size, 12);
    }

    #[test]
    fn sdkwork_cursor_list_query_resolve_page_size_leaves_opaque_cursor_untouched() {
        let query: SdkWorkCursorListQuery =
            serde_urlencoded::from_str("page_size=12&cursor=opaque.jwt.1a2b3c").expect("query");
        // Opaque non-numeric cursors must not be rejected by page-size resolution.
        assert_eq!(query.resolve_page_size().expect("page size"), 12);
        assert_eq!(query.cursor.as_deref(), Some("opaque.jwt.1a2b3c"));
    }

    #[test]
    fn sdkwork_cursor_list_query_resolve_page_size_rejects_out_of_range() {
        let too_large: SdkWorkCursorListQuery =
            serde_urlencoded::from_str("page_size=201").expect("query");
        assert_eq!(
            too_large.resolve_page_size(),
            Err(SdkWorkResultCode::InvalidParameter)
        );
        let too_small: SdkWorkCursorListQuery =
            serde_urlencoded::from_str("page_size=0").expect("query");
        assert_eq!(
            too_small.resolve_page_size(),
            Err(SdkWorkResultCode::InvalidParameter)
        );
        let defaulted: SdkWorkCursorListQuery = serde_urlencoded::from_str("").expect("query");
        assert_eq!(defaulted.resolve_page_size().expect("default"), 20);
    }

    #[derive(Debug, Default, Deserialize)]
    #[serde(rename_all = "camelCase", default)]
    struct FlattenedPageSizeListQuery {
        pub after_audit_seq: Option<u64>,
        #[serde(flatten)]
        pub paging: SdkWorkPageSizeQuery,
    }

    #[test]
    fn flattened_page_size_query_deserializes_from_urlencoded_query_string() {
        let query: FlattenedPageSizeListQuery =
            serde_urlencoded::from_str("afterAuditSeq=0&page_size=2").expect("urlencoded query");
        assert_eq!(query.after_audit_seq, Some(0));
        assert_eq!(query.paging.resolve(), 2);
    }

    #[derive(Debug, Default, Deserialize)]
    #[serde(rename_all = "camelCase", default)]
    struct FlattenedSeqWindowListQuery {
        #[serde(flatten)]
        pub paging: SdkWorkSeqWindowQuery,
    }

    #[test]
    fn sdkwork_seq_window_query_deserializes_after_seq_from_urlencoded_query_string() {
        let query: SdkWorkSeqWindowQuery =
            serde_urlencoded::from_str("afterSeq=0&page_size=2").expect("urlencoded query");
        assert_eq!(query.after_seq, Some(0));
        assert_eq!(query.resolved_page_size(), 2);

        let flattened: FlattenedSeqWindowListQuery =
            serde_urlencoded::from_str("afterSeq=0&page_size=3")
                .expect("flattened urlencoded query");
        assert_eq!(flattened.paging.after_seq, Some(0));
        assert_eq!(flattened.paging.resolved_page_size(), 3);
    }

    #[test]
    fn oauth_protocol_error_body_serializes_rfc6749_shape() {
        let body = OAuthProtocolErrorBody::from_iam_oauth_code(
            "iam_oauth_client_invalid",
            "client_id is required",
        );
        let json = serde_json::to_value(body).expect("serialize");
        assert_eq!(json["error"], "invalid_client");
        assert_eq!(json["error_description"], "client_id is required");
        assert!(json.get("code").is_none());
    }
}
