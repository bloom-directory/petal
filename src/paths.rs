//! Canonical package paths and source-route identities shared with the loader.

use crate::PackageCheckError;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

pub const TAR_NAME_LEN: usize = 100;
const TAR_PREFIX_LEN: usize = 155;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RouteRecord {
    pub route_id: String,
    pub pattern: String,
    pub source_path: PathBuf,
    pub params: Vec<String>,
    pub specificity: RouteSpecificity,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RouteSpecificity {
    pub segment_count: usize,
    pub static_segment_count: usize,
    pub file_score: usize,
}

impl RouteSpecificity {
    pub fn as_array(self) -> [usize; 3] {
        [
            self.segment_count,
            self.static_segment_count,
            self.file_score,
        ]
    }
}

pub fn validate_package_path(path: &str) -> Result<(), PackageCheckError> {
    if path.is_empty()
        || path.starts_with('/')
        || path.contains('\\')
        || path.bytes().any(|b| b == 0)
        || path
            .split('/')
            .any(|segment| segment.is_empty() || segment == "." || segment == "..")
    {
        return Err(PackageCheckError::Path(format!(
            "invalid Petal package path {path:?}"
        )));
    }
    if !path_fits_ustar(path) {
        return Err(PackageCheckError::Path(format!(
            "Petal package path {path:?} is too long for strict .petal.tar archives"
        )));
    }
    Ok(())
}

fn path_fits_ustar(path: &str) -> bool {
    let bytes = path.as_bytes();
    if bytes.len() <= TAR_NAME_LEN {
        return true;
    }
    path.rmatch_indices('/').any(|(idx, _)| {
        idx <= TAR_PREFIX_LEN && bytes.len().saturating_sub(idx + 1) <= TAR_NAME_LEN
    })
}

pub fn route_records_from_paths<'a>(
    paths: impl IntoIterator<Item = &'a str>,
    petal_root: &str,
) -> Result<Vec<RouteRecord>, PackageCheckError> {
    let prefix = format!("{petal_root}/");
    let mut routes = Vec::new();
    let mut has_app_file = false;
    for path in paths {
        if let Some(rel) = path.strip_prefix(&prefix) {
            has_app_file = true;
            if rel.ends_with(".wasm") {
                let pattern = route_pattern_from_rel(rel)?;
                routes.push(RouteRecord {
                    route_id: String::new(),
                    params: route_params(&pattern)?,
                    specificity: specificity(&pattern),
                    pattern,
                    source_path: PathBuf::from(path),
                });
            }
        }
    }
    if !has_app_file {
        return Err(PackageCheckError::Route(format!(
            "Petal package missing {petal_root}/ route root"
        )));
    }
    routes.sort_by(|a, b| a.pattern.as_bytes().cmp(b.pattern.as_bytes()));
    for (idx, route) in routes.iter_mut().enumerate() {
        route.route_id = format!("r{:06}", idx + 1);
    }
    validate_route_conflicts(&routes)?;
    Ok(routes)
}

pub fn validate_single_petal_root<'a>(
    paths: impl IntoIterator<Item = &'a str>,
    expected: &str,
) -> Result<(), PackageCheckError> {
    for path in paths {
        let Some(rest) = path.strip_prefix("petal/") else {
            continue;
        };
        let root = rest.split('/').next().unwrap_or_default();
        if root != expected {
            return Err(PackageCheckError::Route(format!(
                "Petal package has extra petal root {root:?}; expected only petal/{expected}/"
            )));
        }
    }
    Ok(())
}

pub fn route_pattern_from_rel(rel: &str) -> Result<String, PackageCheckError> {
    let mut segments = rel.split('/').collect::<Vec<_>>();
    for segment in &segments {
        let segment = *segment;
        if segment.is_empty() || segment == "." || segment == ".." {
            return Err(PackageCheckError::Route(format!(
                "route path contains invalid segment {segment:?}"
            )));
        }
    }
    if segments.is_empty() {
        return Err(PackageCheckError::Route("empty route path".into()));
    }
    if let Some(reserved) = segments[..segments.len() - 1]
        .iter()
        .find(|segment| segment.starts_with('$'))
    {
        return Err(PackageCheckError::Route(format!(
            "reserved route segment {reserved:?} is only allowed as a recognized route leaf"
        )));
    }
    let last = segments
        .last_mut()
        .expect("route path was checked as non-empty");
    let Some(last_without_wasm) = last.strip_suffix(".wasm") else {
        return Err(PackageCheckError::Route("route leaf is not .wasm".into()));
    };
    match last_without_wasm {
        "$index" => {
            segments.pop();
            Ok(segments.join("/"))
        }
        "$lookup" => {
            *last = last_without_wasm;
            Ok(segments.join("/"))
        }
        other if other.starts_with('$') => Err(PackageCheckError::Route(format!(
            "unsupported reserved route file {other}.wasm"
        ))),
        other => {
            *last = other;
            Ok(segments.join("/"))
        }
    }
}

pub fn route_params(pattern: &str) -> Result<Vec<String>, PackageCheckError> {
    let mut params = Vec::new();
    for segment in pattern.split('/') {
        if let Some((param, _suffix)) = dynamic_segment(segment)? {
            if params.iter().any(|existing| existing == param) {
                return Err(PackageCheckError::Route(format!(
                    "duplicate route param {param:?} in {pattern:?}"
                )));
            }
            params.push(param.to_string());
        }
    }
    Ok(params)
}

pub fn specificity(pattern: &str) -> RouteSpecificity {
    let segments = pattern.split('/').collect::<Vec<_>>();
    RouteSpecificity {
        segment_count: segments.len(),
        static_segment_count: segments
            .iter()
            .filter(|segment| !segment.starts_with('['))
            .count(),
        file_score: usize::from(!pattern.ends_with('/')),
    }
}

pub fn validate_route_conflicts(routes: &[RouteRecord]) -> Result<(), PackageCheckError> {
    for (idx, a) in routes.iter().enumerate() {
        for b in routes.iter().skip(idx + 1) {
            if a.specificity == b.specificity && patterns_overlap(&a.pattern, &b.pattern)? {
                return Err(PackageCheckError::Route(format!(
                    "conflicting Petal routes {:?} and {:?}",
                    a.pattern, b.pattern
                )));
            }
            if file_route_shadows_descendant(a, b)? || file_route_shadows_descendant(b, a)? {
                return Err(PackageCheckError::Route(format!(
                    "file route shadows descendant Petal route: {:?} and {:?}",
                    a.pattern, b.pattern
                )));
            }
        }
    }
    Ok(())
}

fn file_route_shadows_descendant(
    candidate: &RouteRecord,
    descendant: &RouteRecord,
) -> Result<bool, PackageCheckError> {
    if candidate
        .source_path
        .file_name()
        .and_then(|name| name.to_str())
        == Some("$index.wasm")
    {
        return Ok(false);
    }
    let candidate_segments = candidate.pattern.split('/').collect::<Vec<_>>();
    let descendant_segments = descendant.pattern.split('/').collect::<Vec<_>>();
    if candidate_segments.len() >= descendant_segments.len() {
        return Ok(false);
    }
    for (candidate, descendant) in candidate_segments.into_iter().zip(descendant_segments) {
        if !segment_covers(candidate, descendant)? {
            return Ok(false);
        }
    }
    Ok(true)
}

/// Returns whether every path segment matched by `descendant` is also matched
/// by `candidate`. Shadowing needs containment, not mere overlap: a reserved
/// static route such as `new` overlaps `[id]`, but only for one value and must
/// not make all `[id]/...` descendants invalid.
fn segment_covers(candidate: &str, descendant: &str) -> Result<bool, PackageCheckError> {
    match (dynamic_segment(candidate)?, dynamic_segment(descendant)?) {
        (None, None) => Ok(candidate == descendant),
        (Some((_param, suffix)), None) => Ok(descendant
            .strip_suffix(suffix)
            .is_some_and(|bound| !bound.is_empty())),
        (None, Some(_)) => Ok(false),
        (
            Some((_candidate_param, candidate_suffix)),
            Some((_descendant_param, descendant_suffix)),
        ) => Ok(descendant_suffix.ends_with(candidate_suffix)),
    }
}

fn patterns_overlap(a: &str, b: &str) -> Result<bool, PackageCheckError> {
    let a = a.split('/').collect::<Vec<_>>();
    let b = b.split('/').collect::<Vec<_>>();
    if a.len() != b.len() {
        return Ok(false);
    }
    for (a, b) in a.into_iter().zip(b) {
        if !segments_overlap(a, b)? {
            return Ok(false);
        }
    }
    Ok(true)
}

fn segments_overlap(a: &str, b: &str) -> Result<bool, PackageCheckError> {
    match (dynamic_segment(a)?, dynamic_segment(b)?) {
        (None, None) => Ok(a == b),
        (Some((_param, suffix)), None) => Ok(b.ends_with(suffix)),
        (None, Some((_param, suffix))) => Ok(a.ends_with(suffix)),
        (Some((_a_param, a_suffix)), Some((_b_param, b_suffix))) => Ok(a_suffix == b_suffix
            || a_suffix.ends_with(b_suffix)
            || b_suffix.ends_with(a_suffix)),
    }
}

pub fn dynamic_segment(segment: &str) -> Result<Option<(&str, &str)>, PackageCheckError> {
    if !segment.starts_with('[') {
        return Ok(None);
    }
    let Some(end) = segment.find(']') else {
        return Err(PackageCheckError::Route(format!(
            "dynamic route segment missing ]: {segment:?}"
        )));
    };
    let param = &segment[1..end];
    if param.is_empty()
        || !param
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-'))
    {
        return Err(PackageCheckError::Route(format!(
            "invalid route param in segment {segment:?}"
        )));
    }
    Ok(Some((param, &segment[end + 1..])))
}
