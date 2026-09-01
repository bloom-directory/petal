//! Bounded static evidence for owner-approved package registration.
//!
//! Digest claims are not proof that WASM is valid. Machine remains responsible
//! for hashing the actual files, artifact validation, and metadata execution.

use crate::{PackageCheckError, parse_manifest_bounds, paths};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

pub const MAX_FILE_PAGES: usize = 16;
pub const MAX_PAGE_ENTRIES: usize = 256;
pub const MAX_ROUTES: usize = 256;
pub const MAX_PERMISSION_ITEMS: usize = 256;
pub const MAX_MANIFEST_BYTES: usize = 64 * 1024;
pub const MAX_PATH_BYTES: usize = 512;
pub const MAX_REQUEST_BYTES: usize = 768 * 1024;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FileDigestEntry {
    pub path: String,
    #[serde(with = "decimal_u64")]
    pub byte_len: u64,
    pub blake3_hex: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PackageEvidence {
    pub package_hash: String,
    pub file_pages: Vec<Vec<FileDigestEntry>>,
    pub manifest_utf8: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RequestedRoutePermission {
    pub route_id: String,
    pub source_path: String,
    pub capabilities: Vec<String>,
    pub signing_operations: Vec<String>,
    pub key_derive_operations: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CheckedPackageRequest {
    pub evidence: PackageEvidence,
    pub routes: Vec<RequestedRoutePermission>,
}

/// Shared package digest preimage. Input order is preserved for the existing
/// loader's hash adapter; evidence callers use `package_hash_from_entries` to
/// validate and sort claims first.
pub fn hash_file_claims<'a>(
    entries: impl IntoIterator<Item = (&'a str, u64, [u8; 32])>,
) -> Result<String, PackageCheckError> {
    let mut hasher = blake3::Hasher::new();
    hasher.update(crate::PACKAGE_DIGEST_PREFIX);
    for (path, byte_len, digest) in entries {
        let path_len = u32::try_from(path.len()).map_err(|_| {
            PackageCheckError::Limit("path length overflows the package digest format".into())
        })?;
        hasher.update(&path_len.to_le_bytes());
        hasher.update(path.as_bytes());
        hasher.update(&byte_len.to_le_bytes());
        hasher.update(&digest);
    }
    Ok(hasher.finalize().to_hex().to_string())
}

pub fn package_hash_from_entries(entries: &[FileDigestEntry]) -> Result<String, PackageCheckError> {
    let mut sorted = entries.iter().collect::<Vec<_>>();
    sorted.sort_by(|a, b| a.path.as_bytes().cmp(b.path.as_bytes()));
    let mut claims = Vec::with_capacity(sorted.len());
    let mut previous = None;
    for entry in sorted {
        paths::validate_package_path(&entry.path)?;
        if previous == Some(entry.path.as_str()) {
            return Err(PackageCheckError::Path(format!(
                "duplicate Petal package path {:?}",
                entry.path
            )));
        }
        previous = Some(entry.path.as_str());
        claims.push((
            entry.path.as_str(),
            entry.byte_len,
            decode_digest(&entry.blake3_hex)?,
        ));
    }
    hash_file_claims(claims)
}

fn decode_digest(value: &str) -> Result<[u8; 32], PackageCheckError> {
    if value.len() != 64
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        return Err(PackageCheckError::Hash(
            "digest must be 64 lowercase hexadecimal digits".into(),
        ));
    }
    let mut digest = [0; 32];
    hex::decode_to_slice(value, &mut digest)
        .map_err(|error| PackageCheckError::Hash(error.to_string()))?;
    Ok(digest)
}

pub fn check_package_request(
    evidence: PackageEvidence,
    routes: Vec<RequestedRoutePermission>,
) -> Result<CheckedPackageRequest, PackageCheckError> {
    let request = CheckedPackageRequest { evidence, routes };
    check_request_limits(&request)?;
    let evidence = &request.evidence;
    let entries = evidence
        .file_pages
        .iter()
        .flatten()
        .cloned()
        .collect::<Vec<_>>();
    decode_digest(&evidence.package_hash)?;
    if package_hash_from_entries(&entries)? != evidence.package_hash {
        return Err(PackageCheckError::Hash(
            "file claims do not match the package hash".into(),
        ));
    }
    let files = entries
        .iter()
        .map(|entry| (entry.path.as_str(), entry))
        .collect::<BTreeMap<_, _>>();
    for required in ["petal.toml", "README.md", "AGENTS.md"] {
        if !files.contains_key(required) {
            return Err(PackageCheckError::Path(format!(
                "Petal package missing required file {required}"
            )));
        }
    }
    let manifest = files["petal.toml"];
    let manifest_len = u64::try_from(evidence.manifest_utf8.len())
        .map_err(|_| PackageCheckError::Limit("manifest length overflows u64".into()))?;
    if manifest.byte_len != manifest_len
        || manifest.blake3_hex
            != blake3::hash(evidence.manifest_utf8.as_bytes())
                .to_hex()
                .as_str()
    {
        return Err(PackageCheckError::Hash(
            "manifest bytes do not match their file claim".into(),
        ));
    }
    let bounds = parse_manifest_bounds(&evidence.manifest_utf8)?;
    paths::validate_single_petal_root(files.keys().copied(), &bounds.name)?;
    // Bound before quadratic conflict checking, including when the proposal omits routes.
    let prefix = format!("petal/{}/", bounds.name);
    check_limit(
        files
            .keys()
            .filter(|path| path.starts_with(&prefix) && path.ends_with(".wasm"))
            .count(),
        MAX_ROUTES,
        "source routes",
    )?;
    let source_routes =
        paths::route_records_from_paths(files.keys().copied(), &format!("petal/{}", bounds.name))?;
    if source_routes.is_empty() || source_routes.len() != request.routes.len() {
        return Err(PackageCheckError::Route(
            "proposal must include every source route exactly once".into(),
        ));
    }
    let patterns = source_routes
        .iter()
        .map(|route| route.pattern.as_str())
        .collect::<BTreeSet<_>>();
    for pattern in bounds.key_derive.keys() {
        if !patterns.contains(pattern.as_str()) {
            return Err(PackageCheckError::Manifest(format!(
                "Petal [[key.derive]] declares unknown route {pattern:?}"
            )));
        }
    }
    let by_id = source_routes
        .iter()
        .map(|route| (route.route_id.as_str(), route))
        .collect::<BTreeMap<_, _>>();
    let mut seen = BTreeSet::new();
    for route in &request.routes {
        if !seen.insert(route.route_id.as_str()) {
            return Err(PackageCheckError::Route(format!(
                "duplicate route {:?}",
                route.route_id
            )));
        }
        let source = by_id.get(route.route_id.as_str()).ok_or_else(|| {
            PackageCheckError::Route(format!("unknown route {:?}", route.route_id))
        })?;
        if source.source_path.to_str() != Some(route.source_path.as_str()) {
            return Err(PackageCheckError::Route(format!(
                "source path does not match route {:?}",
                route.route_id
            )));
        }
        check_subset(&route.capabilities, &bounds.capabilities, "capabilities")?;
        check_subset(
            &route.signing_operations,
            &bounds.signing_operations,
            "signing operations",
        )?;
        let key_classes = bounds
            .key_derive
            .get(&source.pattern)
            .map(Vec::as_slice)
            .unwrap_or_default();
        check_subset(
            &route.key_derive_operations,
            key_classes,
            "key-derive operations",
        )?;
    }
    Ok(request)
}

fn check_subset(
    requested: &[String],
    allowed: &[String],
    label: &str,
) -> Result<(), PackageCheckError> {
    for value in requested {
        if !allowed.contains(value) {
            return Err(PackageCheckError::Scope(format!(
                "{label} include undeclared value {value:?}"
            )));
        }
    }
    Ok(())
}

fn check_limit(actual: usize, limit: usize, label: &str) -> Result<(), PackageCheckError> {
    if actual > limit {
        return Err(PackageCheckError::Limit(format!("{label} exceeds {limit}")));
    }
    Ok(())
}

fn check_request_limits(request: &CheckedPackageRequest) -> Result<(), PackageCheckError> {
    check_limit(
        request.evidence.file_pages.len(),
        MAX_FILE_PAGES,
        "file pages",
    )?;
    check_limit(request.routes.len(), MAX_ROUTES, "routes")?;
    check_limit(
        request.evidence.manifest_utf8.len(),
        MAX_MANIFEST_BYTES,
        "manifest bytes",
    )?;
    for page in &request.evidence.file_pages {
        check_limit(page.len(), MAX_PAGE_ENTRIES, "file page entries")?;
        for entry in page {
            check_limit(entry.path.len(), MAX_PATH_BYTES, "file path bytes")?;
        }
    }
    for route in &request.routes {
        check_limit(
            route.source_path.len(),
            MAX_PATH_BYTES,
            "route source path bytes",
        )?;
        for list in [
            &route.capabilities,
            &route.signing_operations,
            &route.key_derive_operations,
        ] {
            check_limit(list.len(), MAX_PERMISSION_ITEMS, "permission items")?;
        }
    }
    // The JCS serializer buffers object fields for sorting. Bound raw string
    // bytes first, before allowing that serializer to copy attacker-supplied data.
    let mut raw_bytes = 0usize;
    let mut count_string = |value: &str| -> Result<(), PackageCheckError> {
        raw_bytes = raw_bytes
            .checked_add(value.len())
            .ok_or_else(|| PackageCheckError::Limit("request size overflow".into()))?;
        check_limit(raw_bytes, MAX_REQUEST_BYTES, "request string bytes")
    };
    count_string(&request.evidence.package_hash)?;
    count_string(&request.evidence.manifest_utf8)?;
    for entry in request.evidence.file_pages.iter().flatten() {
        count_string(&entry.path)?;
        count_string(&entry.blake3_hex)?;
    }
    for route in &request.routes {
        count_string(&route.route_id)?;
        count_string(&route.source_path)?;
        for value in route
            .capabilities
            .iter()
            .chain(&route.signing_operations)
            .chain(&route.key_derive_operations)
        {
            count_string(value)?;
        }
    }
    // Include JSON framing and escaping in the canonical evidence-plus-permissions budget.
    let mut writer = BudgetWriter {
        remaining: MAX_REQUEST_BYTES,
    };
    serde_jcs::to_writer(&mut writer, request).map_err(|_| {
        PackageCheckError::Limit(format!(
            "canonical request exceeds {MAX_REQUEST_BYTES} bytes"
        ))
    })?;
    Ok(())
}

struct BudgetWriter {
    remaining: usize,
}

impl std::io::Write for BudgetWriter {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        self.remaining = self
            .remaining
            .checked_sub(bytes.len())
            .ok_or_else(|| std::io::Error::other("canonical request budget exceeded"))?;
        Ok(bytes.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

mod decimal_u64 {
    use serde::{Deserialize, Deserializer, Serializer};

    pub fn serialize<S: Serializer>(value: &u64, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&value.to_string())
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(deserializer: D) -> Result<u64, D::Error> {
        let value = String::deserialize(deserializer)?;
        if value.is_empty()
            || value.len() > 1 && value.starts_with('0')
            || !value.bytes().all(|byte| byte.is_ascii_digit())
        {
            return Err(serde::de::Error::custom(
                "byte_len must be a canonical unsigned decimal string",
            ));
        }
        value.parse().map_err(serde::de::Error::custom)
    }
}
