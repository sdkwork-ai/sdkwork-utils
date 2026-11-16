//! Catalog view filters (`API_SPEC.md` §14.1 list-query vocabulary).
//!
//! Every SDKWork catalog lists the same two optional dimensions, so the wire
//! vocabulary, the normalization, and the visibility predicate live here once
//! instead of in each owning application. A catalog list endpoint adds the two
//! query parameters by embedding [`CatalogViewQuery`] and decides a record's
//! membership by calling [`CatalogViewFilter::allows`] (in-memory filter) or by
//! binding [`CatalogViewFilter::category_code`] / [`CatalogViewFilter::role_codes`]
//! into its store query (SQL push-down). Neither path needs to change this
//! module, which is what keeps a new domain's catalog additive.
//!
//! ## Semantics (the same in every domain)
//!
//! - **category**: one registry category code, matched against the record's
//!   category (or one of its bound categories). The sentinel `all` — and the
//!   localized `全部` the clients ship — means *every* category, as does an
//!   omitted or blank value.
//! - **role scope**: the IAM role codes of the signed-in caller. An empty scope
//!   is the "all data" view. A populated scope keeps the records that name one
//!   of those roles **plus** the records whose list is empty, because an empty
//!   `allowed_role_codes` means "every role" in every SDKWork catalog table.
//! - Neither filter widens visibility: the owning store already scopes rows to
//!   the caller's tenant (and organization/visibility), and both filters only
//!   remove rows from that set.
//!
//! Normalized values are canonical lowercase because category codes and IAM
//! role codes are canonical lowercase identifiers (`NAMING_SPEC.md`), while
//! record comparisons stay case-insensitive so a legacy row cannot silently
//! drop out of the caller's own scope.

use serde::Deserialize;

/// Selector value that keeps every category in a listing.
pub const CATALOG_ALL_CATEGORIES: &str = "all";

/// Localized selector the SDKWork clients ship alongside [`CATALOG_ALL_CATEGORIES`].
const CATALOG_ALL_CATEGORIES_ZH: &str = "全部";

/// Normalizes a category selector to `None` (every category) or one code.
///
/// The result is canonical lowercase; record matches stay case-insensitive so
/// an older row written with different casing still matches.
pub fn normalize_category_filter(value: Option<&str>) -> Option<String> {
    let trimmed = value?.trim();
    if trimmed.is_empty()
        || trimmed.eq_ignore_ascii_case(CATALOG_ALL_CATEGORIES)
        || trimmed == CATALOG_ALL_CATEGORIES_ZH
    {
        return None;
    }
    Some(trimmed.to_ascii_lowercase())
}

/// Parses a comma-separated `role_codes` value into a sorted, unique scope.
///
/// Codes are IAM role codes (`NAMING_SPEC.md` role-code pattern); blank entries
/// are dropped rather than rejected so a trailing comma stays harmless.
pub fn normalize_role_code_filter(value: Option<&str>) -> Vec<String> {
    let Some(value) = value else {
        return Vec::new();
    };
    normalize_role_code_values(
        &value
            .split(',')
            .map(str::to_string)
            .collect::<Vec<String>>(),
    )
}

/// Normalizes an already-split role-code list: trim, lowercase, sort, dedupe.
fn normalize_role_code_values(values: &[String]) -> Vec<String> {
    let mut codes = values
        .iter()
        .map(|code| code.trim())
        .filter(|code| !code.is_empty())
        .map(str::to_ascii_lowercase)
        .collect::<Vec<String>>();
    codes.sort();
    codes.dedup();
    codes
}

/// True when a record's category matches the normalized category filter.
///
/// A record without a category never matches a specific category; it still
/// appears in the all-category view because that view passes `None`.
pub fn category_matches(record_category: Option<&str>, category: &str) -> bool {
    record_category.is_some_and(|value| value.eq_ignore_ascii_case(category))
}

/// True when any of a record's bound categories matches the filter.
///
/// Catalogs whose records may bind several categories (Skills packages) use
/// this form; single-category catalogs use [`category_matches`].
pub fn category_list_matches(record_categories: &[String], category: &str) -> bool {
    record_categories
        .iter()
        .any(|value| value.eq_ignore_ascii_case(category))
}

/// True when a record restricted to `allowed` is part of the caller's scope.
///
/// An empty `role_codes` is the "all data" view and matches everything. An
/// empty `allowed` list means the record is open to every role, so it matches
/// every scope even when the caller narrows the listing by role.
pub fn role_scope_allows(allowed: &[String], role_codes: &[String]) -> bool {
    role_codes.is_empty()
        || allowed.is_empty()
        || allowed.iter().any(|code| {
            role_codes
                .iter()
                .any(|scope| scope.eq_ignore_ascii_case(code))
        })
}

/// One normalized catalog view: the category selector plus the caller's roles.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CatalogViewFilter {
    category_code: Option<String>,
    role_codes: Vec<String>,
}

impl CatalogViewFilter {
    /// The "all data" view: every category, every role.
    pub fn all_data() -> Self {
        Self::default()
    }

    /// Builds the filter from raw query values.
    pub fn from_query(category_code: Option<&str>, role_codes: Option<&str>) -> Self {
        Self {
            category_code: normalize_category_filter(category_code),
            role_codes: normalize_role_code_filter(role_codes),
        }
    }

    /// Builds the filter from values that are already normalized.
    ///
    /// Store predicates bind the normalized values directly, so this keeps one
    /// constructor for both the request path and a pre-normalized caller.
    pub fn from_normalized(category_code: Option<String>, role_codes: Vec<String>) -> Self {
        Self {
            category_code: normalize_category_filter(category_code.as_deref()),
            role_codes: normalize_role_code_values(&role_codes),
        }
    }

    /// One category code, or `None` for every category.
    pub fn category_code(&self) -> Option<&str> {
        self.category_code.as_deref()
    }

    /// Normalized IAM role codes of the caller; empty is the all-data view.
    pub fn role_codes(&self) -> &[String] {
        &self.role_codes
    }

    /// True when this filter keeps every record.
    pub fn is_all_data(&self) -> bool {
        self.category_code.is_none() && self.role_codes.is_empty()
    }

    /// True when a single-category record satisfies the category selector.
    pub fn allows_category(&self, record_category: Option<&str>) -> bool {
        match self.category_code.as_deref() {
            Some(code) => category_matches(record_category, code),
            None => true,
        }
    }

    /// True when a multi-category record satisfies the category selector.
    pub fn allows_any_category(&self, record_categories: &[String]) -> bool {
        match self.category_code.as_deref() {
            Some(code) => category_list_matches(record_categories, code),
            None => true,
        }
    }

    /// True when a record's `allowed_role_codes` are inside the caller's scope.
    pub fn allows_roles(&self, allowed_role_codes: &[String]) -> bool {
        role_scope_allows(allowed_role_codes, &self.role_codes)
    }

    /// True when a single-category record is inside both halves of the view.
    pub fn allows(&self, record_category: Option<&str>, allowed_role_codes: &[String]) -> bool {
        self.allows_category(record_category) && self.allows_roles(allowed_role_codes)
    }

    /// True when a multi-category record is inside both halves of the view.
    pub fn allows_any(&self, record_categories: &[String], allowed_role_codes: &[String]) -> bool {
        self.allows_any_category(record_categories) && self.allows_roles(allowed_role_codes)
    }
}

/// The catalog view query parameters (`category_code`, `role_codes`).
///
/// Embed this in a list-query DTO with `#[serde(flatten)]` so every catalog
/// endpoint accepts the same URL vocabulary without restating the fields.
#[derive(Debug, Default, Clone, Deserialize)]
pub struct CatalogViewQuery {
    /// One registry category code; `all` (or blank) keeps every category.
    #[serde(alias = "categoryCode")]
    pub category_code: Option<String>,
    /// Comma-separated IAM role codes of the signed-in caller.
    #[serde(alias = "roleCodes")]
    pub role_codes: Option<String>,
}

impl CatalogViewQuery {
    /// Normalized filter for this query.
    pub fn to_filter(&self) -> CatalogViewFilter {
        CatalogViewFilter::from_query(self.category_code.as_deref(), self.role_codes.as_deref())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn codes(values: &[&str]) -> Vec<String> {
        values.iter().map(|value| value.to_string()).collect()
    }

    #[test]
    fn category_filter_drops_the_all_selectors() {
        assert_eq!(normalize_category_filter(None), None);
        assert_eq!(normalize_category_filter(Some("  ")), None);
        assert_eq!(normalize_category_filter(Some("ALL")), None);
        assert_eq!(normalize_category_filter(Some("全部")), None);
        assert_eq!(
            normalize_category_filter(Some(" Code-Development ")),
            Some("code-development".to_string())
        );
    }

    #[test]
    fn role_code_filter_trims_sorts_lowercases_and_dedupes() {
        assert_eq!(normalize_role_code_filter(None), Vec::<String>::new());
        assert_eq!(
            normalize_role_code_filter(Some("Org_Admin, org_operations,,ORG_ADMIN,")),
            vec!["org_admin".to_string(), "org_operations".to_string()]
        );
    }

    #[test]
    fn role_scope_matches_open_records_and_named_roles() {
        let all_data: Vec<String> = Vec::new();
        let operator = codes(&["org_admin"]);
        let open: Vec<String> = Vec::new();
        let restricted = codes(&["org_finance"]);

        assert!(role_scope_allows(&restricted, &all_data));
        assert!(role_scope_allows(&open, &operator));
        assert!(role_scope_allows(&["ORG_ADMIN".to_string()], &operator));
        assert!(!role_scope_allows(&restricted, &operator));
    }

    #[test]
    fn category_matching_is_case_insensitive_for_one_and_many_categories() {
        assert!(category_matches(
            Some("Code-Development"),
            "code-development"
        ));
        assert!(!category_matches(
            Some("data-retrieval"),
            "code-development"
        ));
        assert!(!category_matches(None, "code-development"));

        assert!(category_list_matches(
            &codes(&["CODING", "writing"]),
            "coding"
        ));
        assert!(!category_list_matches(&codes(&["writing"]), "coding"));
        assert!(!category_list_matches(&[], "coding"));
    }

    #[test]
    fn filter_composes_category_and_role_scope() {
        let filter = CatalogViewFilter::from_query(Some("coding"), Some("org_auditor"));

        assert!(!filter.is_all_data());
        assert!(filter.allows(Some("coding"), &[]));
        assert!(filter.allows(Some("CODING"), &codes(&["org_auditor"])));
        assert!(!filter.allows(Some("coding"), &codes(&["org_finance"])));
        assert!(!filter.allows(Some("writing"), &[]));
        assert!(!filter.allows_category(None));

        let all_data = CatalogViewFilter::from_query(Some("all"), Some(" "));
        assert!(all_data.is_all_data());
        assert!(all_data.allows(Some("writing"), &codes(&["org_finance"])));
    }

    #[test]
    fn filter_matches_multi_category_records() {
        let filter = CatalogViewFilter::from_query(Some("coding"), None);
        assert!(filter.allows_any(&codes(&["writing", "coding"]), &[]));
        assert!(!filter.allows_any(&codes(&["writing"]), &[]));
        assert!(CatalogViewFilter::all_data().allows_any(&codes(&["writing"]), &[]));
    }

    #[test]
    fn from_normalized_is_idempotent() {
        let filter = CatalogViewFilter::from_normalized(
            Some("Coding".to_string()),
            codes(&["Org_Admin", "org_admin"]),
        );
        assert_eq!(filter.category_code(), Some("coding"));
        assert_eq!(filter.role_codes(), codes(&["org_admin"]).as_slice());
    }

    #[test]
    fn query_parses_the_camel_case_aliases() {
        let query: CatalogViewQuery = serde_json::from_str(
            r#"{"categoryCode":" Coding ","roleCodes":"org_admin,org_operations"}"#,
        )
        .expect("catalog view query parses");
        let filter = query.to_filter();
        assert_eq!(filter.category_code(), Some("coding"));
        assert_eq!(
            filter.role_codes(),
            codes(&["org_admin", "org_operations"]).as_slice()
        );
    }

    #[test]
    fn query_parses_the_url_wire_form() {
        let query: CatalogViewQuery =
            serde_urlencoded::from_str("category_code=all&role_codes=org_admin%2Corg_auditor")
                .expect("query string parses");
        let filter = query.to_filter();
        assert_eq!(filter.category_code(), None);
        assert_eq!(
            filter.role_codes(),
            codes(&["org_admin", "org_auditor"]).as_slice()
        );
    }
}
