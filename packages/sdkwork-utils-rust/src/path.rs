pub fn join_path(segments: &[&str]) -> String {
    segments
        .iter()
        .map(|segment| segment.trim_matches('/'))
        .filter(|segment| !segment.is_empty())
        .collect::<Vec<_>>()
        .join("/")
}

pub fn normalize_path(value: &str) -> String {
    let joined = value
        .split('/')
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>()
        .join("/");
    if value.starts_with('/') {
        format!("/{joined}")
    } else {
        joined
    }
}

/// Collapse duplicate surface prefixes in a path.
///
/// When a gateway mounts a surface at `/app/v3/api/generations` and the upstream
/// also prefixes its routes with the same surface path, the inbound path can
/// arrive doubled. This function detects and collapses the duplication so that
/// downstream routing sees a single canonical prefix.
pub fn collapse_duplicate_surface_prefix(path_and_query: &str) -> String {
    let (path, query) = match path_and_query.split_once('?') {
        Some((path, query)) => (path, Some(query)),
        None => (path_and_query, None),
    };

    let segments: Vec<&str> = path.split('/').filter(|s| !s.is_empty()).collect();
    if segments.len() < 4 {
        return path_and_query.to_string();
    }

    // Try to find a duplicated prefix by checking if the first half matches the second half.
    for prefix_len in (2..=segments.len() / 2).rev() {
        let prefix = &segments[..prefix_len];
        let remainder = &segments[prefix_len..];
        if remainder.starts_with(prefix) {
            let collapsed = remainder.join("/");
            let result = format!("/{collapsed}");
            return match query {
                Some(q) => format!("{result}?{q}"),
                None => result,
            };
        }
    }

    path_and_query.to_string()
}

#[cfg(test)] // WORKSPACE-PATH:allow-fixture-block: this module is the file's #[cfg(test)] unit-test fixture data
mod tests {
    use super::*;

    #[test]
    fn path_helpers() {
        assert_eq!(join_path(&["a", "", "/b/", "c"]), "a/b/c");
        assert_eq!(normalize_path("//a//b//"), "/a/b");
    }

    #[test]
    fn collapse_duplicate_surface_prefix_noop_on_clean_path() {
        assert_eq!(
            collapse_duplicate_surface_prefix("/app/v3/api/generations"),
            "/app/v3/api/generations"
        );
    }

    #[test]
    fn collapse_duplicate_surface_prefix_collapses_doubled_prefix() {
        assert_eq!(
            collapse_duplicate_surface_prefix("/app/v3/api/generations/app/v3/api/generations/123"),
            "/app/v3/api/generations/123"
        );
    }

    #[test]
    fn collapse_duplicate_surface_prefix_preserves_query() {
        assert_eq!(
            collapse_duplicate_surface_prefix(
                "/app/v3/api/generations/app/v3/api/generations?cursor=abc"
            ),
            "/app/v3/api/generations?cursor=abc"
        );
    }
}
