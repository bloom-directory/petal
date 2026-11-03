use std::collections::{BTreeMap, BTreeSet};
use std::ffi::OsStr;
use std::fs;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use wasmparser::{ComponentExternalKind, Parser, Payload};

const ALL_CAPS: &[&str] = &[
    "bloom:http",
    "bloom:store",
    "bloom:sign",
    "bloom:tx.outbox",
    "bloom:chain",
    "bloom:vfs.read",
    "bloom:vfs.write",
];

#[derive(Clone, Debug)]
struct Route {
    path: String,
    canonical: String,
    source: PathBuf,
    package: String,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BuildConfig {
    pub schema: String,
    pub name: String,
    pub routes: PathBuf,
    pub output: PathBuf,
    pub route_crate: RouteCrate,
    #[serde(default)]
    pub sdk: SdkDependency,
    #[serde(default)]
    pub dependencies: BTreeMap<String, toml::Value>,
    #[serde(default)]
    pub jobs: Option<usize>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RouteCrate {
    pub package: String,
    pub path: PathBuf,
    pub alias: String,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SdkDependency {
    #[serde(default = "default_sdk_package")]
    pub package: String,
    #[serde(default)]
    pub version: Option<String>,
    #[serde(default)]
    pub path: Option<PathBuf>,
    #[serde(default)]
    pub git: Option<String>,
    #[serde(default)]
    pub rev: Option<String>,
}

impl Default for SdkDependency {
    fn default() -> Self {
        Self {
            package: default_sdk_package(),
            version: None,
            path: None,
            git: None,
            rev: None,
        }
    }
}

#[derive(Clone, Debug, Serialize)]
pub struct BuildReport {
    pub schema: &'static str,
    pub petal: String,
    pub contract: &'static str,
    pub wit_digest: String,
    pub routes: usize,
    pub output: PathBuf,
    pub artifacts: BTreeMap<String, String>,
    pub cargo_version: String,
    pub rustc_version: String,
    pub wasm_tools_version: String,
}

#[derive(Clone, Debug, Serialize)]
pub struct PackageReport {
    pub schema: &'static str,
    pub petal: String,
    pub contract: &'static str,
    pub wit_digest: String,
    pub routes: usize,
    pub files: usize,
    pub bytes: u64,
    pub package_hash: String,
    pub archive: PathBuf,
    pub sha256: String,
}

#[derive(Debug, Deserialize)]
struct PackageManifest {
    schema: String,
    name: String,
}

#[derive(Debug)]
struct PackageFile {
    path: String,
    bytes: Vec<u8>,
}

fn default_sdk_package() -> String {
    "bloom-petal-sdk".into()
}

pub fn load_config(path: impl AsRef<Path>) -> Result<BuildConfig, String> {
    let path = path.as_ref();
    let body = fs::read_to_string(path).map_err(display_err("read", path))?;
    let config: BuildConfig =
        toml::from_str(&body).map_err(|error| format!("parse {}: {error}", path.display()))?;
    if config.schema != "bloom.petal.build.v1" {
        return Err(format!(
            "{} must set schema = \"bloom.petal.build.v1\"",
            path.display()
        ));
    }
    validate_name(&config.name)?;
    validate_relative(&config.routes, "routes")?;
    validate_relative(&config.output, "output")?;
    validate_relative(&config.route_crate.path, "route_crate.path")?;
    validate_alias(&config.route_crate.alias)?;
    if config.sdk.version.is_some()
        || config.sdk.path.is_some()
        || config.sdk.git.is_some()
        || config.sdk.rev.is_some()
    {
        validate_sdk(&config.sdk)?;
    }
    Ok(config)
}

pub fn build(root: impl AsRef<Path>, config: &BuildConfig) -> Result<BuildReport, String> {
    build_inner(root.as_ref(), config, false)
}

pub fn check_caps(root: impl AsRef<Path>, config: &BuildConfig) -> Result<BuildReport, String> {
    build_inner(root.as_ref(), config, true)
}

/// The optional directories of Bloom's package format. Route sidecars under
/// `petal/<name>/` compose from `components/` and `modules/`; `wit/` holds the
/// package's WIT.
const OPTIONAL_PACKAGE_DIRECTORIES: [&str; 3] = ["components", "modules", "wit"];

/// Every directory tree `petal package` collects files from.
const PACKAGED_DIRECTORIES: [&str; 4] = ["petal", "components", "modules", "wit"];

/// Create the strict, deterministic `.petal.tar.gz` consumed by Bloom.
///
/// Callers should build and check their routes first. The CLI's `package`
/// command rebuilds current sources before calling this function, or with
/// `--no-build` checks the components a Petal's own build command produced.
/// Machine composition and installation apply the host's authoritative
/// validation.
pub fn package(
    root: impl AsRef<Path>,
    config: &BuildConfig,
    out: impl AsRef<Path>,
) -> Result<PackageReport, String> {
    let root = root
        .as_ref()
        .canonicalize()
        .map_err(|error| format!("resolve {}: {error}", root.as_ref().display()))?;
    check_sdk_pin(&root, config)?;
    let out = absolute_from(&root, out.as_ref());
    // Bloom reads these under exactly these names, so a case-insensitive file
    // system must not find them under another.
    for required in ["petal.toml", "README.md", "AGENTS.md"] {
        match exact_entry(&root, Path::new(required), "Petal package")? {
            Some(path)
                if fs::symlink_metadata(&path)
                    .map_err(display_err("inspect", &path))?
                    .is_file() => {}
            Some(path) => {
                return Err(format!(
                    "Petal package {} is not a regular file",
                    path.display()
                ));
            }
            None => return Err(format!("Petal package missing required file {required}")),
        }
    }
    let manifest_path = root.join("petal.toml");
    let manifest_body =
        fs::read_to_string(&manifest_path).map_err(display_err("read", &manifest_path))?;
    let manifest: PackageManifest = toml::from_str(&manifest_body)
        .map_err(|error| format!("parse {}: {error}", manifest_path.display()))?;
    if manifest.schema != bloom_petal_contract::PACKAGE_SCHEMA {
        return Err(format!(
            "{} must set schema = {:?}",
            manifest_path.display(),
            bloom_petal_contract::PACKAGE_SCHEMA
        ));
    }
    if manifest.name != config.name {
        return Err(format!(
            "petal.toml name {:?} does not match build name {:?}",
            manifest.name, config.name
        ));
    }
    let runtime = PathBuf::from("petal").join(&config.name);
    if config.output != runtime {
        return Err(format!(
            "installable package output must be {}, got {}",
            runtime.display(),
            config.output.display()
        ));
    }
    let no_routes = || {
        format!(
            "Petal package contains no route components under {}",
            config.output.display()
        )
    };
    let runtime_dir =
        exact_entry(&root, &runtime, "Petal package runtime")?.ok_or_else(no_routes)?;
    let routes = discover_wasm_paths(&runtime_dir)?;
    if routes.is_empty() {
        return Err(no_routes());
    }
    // Keep archives out of the packaged trees, comparing the path the file
    // system will resolve: through links, `..`, and on macOS letter case,
    // where `PETAL/x` lands in `petal/`.
    let resolved_out = resolve_existing(&out);
    for directory in PACKAGED_DIRECTORIES {
        if resolved_out.starts_with(root.join(directory)) {
            return Err(format!(
                "archive output must be outside the packaged {directory}/ tree"
            ));
        }
    }
    // These are the host-consumed inputs. Keep all runtime files and the full
    // security manifest; development reports and source bookkeeping are not
    // installation identity. Machine composition still assigns its own hash.
    // Bloom installs only petal/<name>/ and refuses any other entry under
    // petal/, such as a .DS_Store or a renamed Petal's old tree.
    let mut files = Vec::new();
    for path in ["petal.toml", "README.md", "AGENTS.md"] {
        collect_package_files(&root, &root.join(path), &mut files)?;
    }
    collect_package_files(&root, &runtime_dir, &mut files)?;
    for directory in OPTIONAL_PACKAGE_DIRECTORIES {
        if let Some(path) = exact_entry(&root, Path::new(directory), "Petal package")? {
            // Bloom reads these as directories; a file of the same name would
            // be packaged as one entry that nothing composes from. Links and
            // other non-regular files are refused as anywhere else.
            if fs::symlink_metadata(&path)
                .map_err(display_err("inspect", &path))?
                .is_file()
            {
                return Err(format!(
                    "Petal package {directory} must be a directory: {}",
                    path.display()
                ));
            }
            collect_package_files(&root, &path, &mut files)?;
        }
    }
    // Generated scaffolds point AGENTS.md at this pinned authoring document.
    // Retain it when present, including rejecting linked or unreadable copies.
    if let Some(skill) = packaged_skill(&root)? {
        collect_package_files(&root, &skill, &mut files)?;
    }
    files.sort_by(|left, right| left.path.as_bytes().cmp(right.path.as_bytes()));
    let package_hash = package_hash(&files);
    let bytes = files.iter().map(|file| file.bytes.len() as u64).sum();

    let parent = out
        .parent()
        .ok_or_else(|| format!("archive output has no parent: {}", out.display()))?;
    fs::create_dir_all(parent).map_err(display_err("create", parent))?;
    if out.exists() {
        return Err(format!("refusing to overwrite archive: {}", out.display()));
    }
    let file_name = out
        .file_name()
        .and_then(OsStr::to_str)
        .ok_or_else(|| format!("archive output is not UTF-8: {}", out.display()))?;
    if !file_name.ends_with(".petal.tar.gz") {
        return Err("archive output must end in .petal.tar.gz".into());
    }
    let temporary = parent.join(format!(".{file_name}.tmp.{}", std::process::id()));
    if temporary.exists() {
        fs::remove_file(&temporary).map_err(display_err("remove", &temporary))?;
    }
    let result = write_package_archive(&temporary, &files);
    if let Err(error) = result {
        let _ = fs::remove_file(&temporary);
        return Err(error);
    }
    fs::rename(&temporary, &out)
        .map_err(|error| format!("install archive {}: {error}", out.display()))?;
    let archive_bytes = fs::read(&out).map_err(display_err("read", &out))?;
    let sha256 = hex_sha256(&archive_bytes);
    Ok(PackageReport {
        schema: "bloom.petal.package-report.v1",
        petal: config.name.clone(),
        contract: bloom_petal_contract::ROUTE_PACKAGE,
        wit_digest: bloom_petal_contract::wit_digest(),
        routes: routes.len(),
        files: files.len(),
        bytes,
        package_hash,
        archive: out,
        sha256,
    })
}

/// The development skill `petal new` wrote under `root`, if present, as a
/// regular file, so a directory cannot pull in whatever it holds.
fn packaged_skill(root: &Path) -> Result<Option<PathBuf>, String> {
    let skill = Path::new(bloom_petal_contract::PETAL_DEVELOPMENT_SKILL_PATH);
    let Some(path) = exact_entry(root, skill, "Petal package skill")? else {
        return Ok(None);
    };
    if !fs::symlink_metadata(&path)
        .map_err(display_err("inspect", &path))?
        .is_file()
    {
        return Err(format!(
            "Petal package skill {} is not a regular file",
            path.display()
        ));
    }
    Ok(Some(path))
}

/// `relative` under `root`, if every component exists under exactly that
/// name and every directory on the way is a real directory. A link cannot
/// pull a file into the package from outside the Petal, and a case-insensitive
/// file system cannot find a differently cased name that a case-sensitive one
/// ignores. The caller checks what the final entry is.
fn exact_entry(root: &Path, relative: &Path, what: &str) -> Result<Option<PathBuf>, String> {
    let components = relative.components().collect::<Vec<_>>();
    let mut path = root.to_owned();
    for (index, component) in components.iter().enumerate() {
        if !has_entry(&path, component.as_os_str())? {
            return Ok(None);
        }
        path.push(component);
        if index + 1 < components.len()
            && !fs::symlink_metadata(&path)
                .map_err(display_err("inspect", &path))?
                .is_dir()
        {
            return Err(format!(
                "{what} path crosses non-directory {}",
                path.display()
            ));
        }
    }
    Ok(Some(path))
}

fn has_entry(directory: &Path, name: &OsStr) -> Result<bool, String> {
    for entry in fs::read_dir(directory).map_err(display_err("read", directory))? {
        if entry.map_err(|error| error.to_string())?.file_name() == name {
            return Ok(true);
        }
    }
    Ok(false)
}

fn absolute_from(root: &Path, path: &Path) -> PathBuf {
    if path.is_absolute() {
        path.to_owned()
    } else {
        root.join(path)
    }
}

/// `path` as the file system will see it once the rest is created: its
/// longest existing prefix resolved through links (which on macOS also fixes
/// letter case), and the rest, which will be created as plain directories,
/// with `.` and `..` applied. Applying `..` past a directory that does not
/// exist yet can reach an existing link, so this repeats until the path
/// stops changing.
fn resolve_existing(path: &Path) -> PathBuf {
    let once = |path: &Path| {
        for ancestor in path.ancestors() {
            if let Ok(resolved) = ancestor.canonicalize() {
                return lexical_normalize(
                    &resolved.join(path.strip_prefix(ancestor).unwrap_or(Path::new(""))),
                );
            }
        }
        lexical_normalize(path)
    };
    let mut path = once(path);
    // Each pass resolves at least one more component, so a path settles
    // within as many passes as it has components.
    for _ in 0..path.components().count() {
        let next = once(&path);
        if next == path {
            break;
        }
        path = next;
    }
    path
}

/// `path` with `.` and `..` components applied without touching the file
/// system.
fn lexical_normalize(path: &Path) -> PathBuf {
    let mut normalized = PathBuf::new();
    for component in path.components() {
        match component {
            std::path::Component::CurDir => {}
            std::path::Component::ParentDir => match normalized.components().next_back() {
                Some(std::path::Component::Normal(_)) => {
                    normalized.pop();
                }
                // `..` above the root stays at the root.
                Some(std::path::Component::RootDir | std::path::Component::Prefix(_)) => {}
                _ => normalized.push(".."),
            },
            other => normalized.push(other),
        }
    }
    normalized
}

fn collect_package_files(
    root: &Path,
    path: &Path,
    files: &mut Vec<PackageFile>,
) -> Result<(), String> {
    let metadata = fs::symlink_metadata(path).map_err(display_err("inspect", path))?;
    if metadata.is_dir() {
        let mut entries = fs::read_dir(path)
            .map_err(display_err("read", path))?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|error| error.to_string())?;
        entries.sort_by_key(|entry| entry.file_name());
        for entry in entries {
            collect_package_files(root, &entry.path(), files)?;
        }
    } else if metadata.is_file() {
        // An archive is never package content, wherever it was written.
        if path
            .file_name()
            .and_then(OsStr::to_str)
            .is_some_and(|name| name.ends_with(".petal.tar") || name.ends_with(".petal.tar.gz"))
        {
            return Ok(());
        }
        let relative = package_path(path.strip_prefix(root).map_err(|error| error.to_string())?)?;
        validate_ustar_path(&relative)?;
        files.push(PackageFile {
            path: relative,
            bytes: fs::read(path).map_err(display_err("read", path))?,
        });
    } else {
        return Err(format!(
            "Petal package contains non-regular file {}",
            path.display()
        ));
    }
    Ok(())
}

fn package_path(path: &Path) -> Result<String, String> {
    let parts = path
        .components()
        .map(|component| {
            component
                .as_os_str()
                .to_str()
                .ok_or_else(|| format!("package path is not UTF-8: {}", path.display()))
        })
        .collect::<Result<Vec<_>, _>>()?;
    Ok(parts.join("/"))
}

fn validate_ustar_path(path: &str) -> Result<(), String> {
    const NAME_LEN: usize = 100;
    const PREFIX_LEN: usize = 155;
    if path.is_empty()
        || path.starts_with('/')
        || path.contains('\\')
        || path
            .split('/')
            .any(|part| part.is_empty() || matches!(part, "." | ".."))
    {
        return Err(format!("invalid Petal package path {path:?}"));
    }
    if path.len() <= NAME_LEN
        || path.rmatch_indices('/').any(|(index, _)| {
            index <= PREFIX_LEN && path.len().saturating_sub(index + 1) <= NAME_LEN
        })
    {
        Ok(())
    } else {
        Err(format!(
            "Petal package path {path:?} is too long for strict .petal.tar archives"
        ))
    }
}

fn package_hash(files: &[PackageFile]) -> String {
    let mut hasher = blake3::Hasher::new();
    hasher.update(bloom_petal_contract::PACKAGE_DIGEST_PREFIX);
    for file in files {
        hasher.update(&(file.path.len() as u32).to_le_bytes());
        hasher.update(file.path.as_bytes());
        hasher.update(&(file.bytes.len() as u64).to_le_bytes());
        hasher.update(blake3::hash(&file.bytes).as_bytes());
    }
    hasher.finalize().to_hex().to_string()
}

fn write_package_archive(path: &Path, files: &[PackageFile]) -> Result<(), String> {
    let file = fs::File::create(path).map_err(display_err("create", path))?;
    let gzip = flate2::GzBuilder::new()
        .mtime(0)
        .write(file, flate2::Compression::best());
    let mut tar = tar::Builder::new(gzip);
    for package_file in files {
        let mut header = tar::Header::new_ustar();
        header.set_size(package_file.bytes.len() as u64);
        header.set_mode(0o644);
        header.set_uid(0);
        header.set_gid(0);
        header.set_mtime(0);
        header.set_entry_type(tar::EntryType::Regular);
        header.set_cksum();
        tar.append_data(
            &mut header,
            &package_file.path,
            package_file.bytes.as_slice(),
        )
        .map_err(|error| format!("write archive entry {}: {error}", package_file.path))?;
    }
    let gzip = tar
        .into_inner()
        .map_err(|error| format!("finish tar: {error}"))?;
    gzip.finish()
        .map_err(|error| format!("finish gzip: {error}"))?;
    Ok(())
}

fn hex_sha256(bytes: &[u8]) -> String {
    hex::encode(Sha256::digest(bytes))
}

fn build_inner(root: &Path, config: &BuildConfig, check_only: bool) -> Result<BuildReport, String> {
    let root = root
        .canonicalize()
        .map_err(|error| format!("resolve {}: {error}", root.display()))?;
    check_sdk_pin(&root, config)?;
    check_shared_route_architecture(&root, config)?;
    let routes = discover_routes(&root.join(&config.routes))?;
    if routes.is_empty() {
        return Err("no route controllers found".into());
    }
    warn_route_shapes(&routes);
    let out_dir = root.join(&config.output);

    if check_only {
        let mut failures = routes
            .iter()
            .filter_map(|route| {
                check_route_caps(route, &out_dir.join(format!("{}.wasm", route.path))).err()
            })
            .collect::<Vec<_>>();
        // Bloom installs every component in the output as a route, so a
        // leftover one would bring back a route that has no controller.
        if out_dir.is_dir() {
            let expected = routes
                .iter()
                .map(|route| format!("{}.wasm", route.path))
                .collect::<BTreeSet<_>>();
            failures.extend(
                discover_wasm_paths(&out_dir)?
                    .difference(&expected)
                    .map(|path| {
                        format!(
                            "{} has no route controller; rebuild to remove it",
                            out_dir.join(path).display()
                        )
                    }),
            );
        }
        if failures.is_empty() {
            eprintln!(
                "checked capability metadata for {} route components",
                routes.len()
            );
            return report(config, &routes, &out_dir);
        }
        return Err(failures.join("\n"));
    }

    require_tool("cargo")?;
    require_tool("wasm-tools")?;
    let build_root = root.join("target/petal-routes");
    let workspace = build_root.join("workspace");
    generate_workspace(&root, &workspace, &routes, config)?;
    let locked_sources_only =
        !config_overrides_allowed(std::env::var_os("PETAL_REQUIRE_LOCKED_SOURCES"))?;
    reuse_route_lockfile(&root, &workspace, config, &routes, locked_sources_only)?;
    let artifacts = build_workspace(&root, &workspace, &routes, config.jobs)?;

    let generation = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_err(|error| error.to_string())?
        .as_nanos();
    let staging = build_root.join("staging").join(format!(
        "{}-{}-{generation}",
        config.name,
        std::process::id()
    ));
    let backup = install_generation(&staging, &out_dir, &routes, |route, output| {
        let core = artifacts
            .get(&route.package)
            .ok_or_else(|| format!("no artifact for {}", route.package))?;
        command(
            Command::new("wasm-tools")
                .args(["component", "new"])
                .arg(core)
                .arg("-o")
                .arg(output),
        )?;
        command(Command::new("wasm-tools").arg("validate").arg(output))?;
        check_route_caps(route, output)
    })?;
    eprintln!(
        "wrote {} route components under {}",
        routes.len(),
        out_dir.display()
    );
    // The reports still describe the output just replaced. Older builders
    // update only the legacy build-report.json, so it is tried as well.
    let report_path = build_root.join("build-report.json");
    let output_report = output_report_path(&build_root, config);
    if let Some(backup) = backup {
        retire_backup(&backup, &[&output_report, &report_path], config);
    }
    let report = report(config, &routes, &out_dir)?;
    let body = serde_json::to_string_pretty(&report).map_err(|error| error.to_string())?;
    write_if_changed(&report_path, &body)?;
    let reports = output_report.parent().ok_or("report path has no parent")?;
    fs::create_dir_all(reports).map_err(display_err("create", reports))?;
    write_if_changed(&output_report, &body)?;
    Ok(report)
}

fn report(config: &BuildConfig, routes: &[Route], output: &Path) -> Result<BuildReport, String> {
    let mut artifacts = BTreeMap::new();
    for route in routes {
        let relative = format!("{}.wasm", route.path);
        let bytes = fs::read(output.join(&relative))
            .map_err(display_err("read", &output.join(&relative)))?;
        artifacts.insert(relative, blake3::hash(&bytes).to_hex().to_string());
    }
    Ok(BuildReport {
        schema: "bloom.petal.build-report.v1",
        petal: config.name.clone(),
        contract: bloom_petal_contract::ROUTE_PACKAGE,
        wit_digest: bloom_petal_contract::wit_digest(),
        routes: routes.len(),
        output: config.output.clone(),
        artifacts,
        cargo_version: tool_version("cargo")?,
        rustc_version: tool_version("rustc")?,
        wasm_tools_version: tool_version("wasm-tools")?,
    })
}

/// The release profile route components build with. One codegen unit keeps
/// function order independent of the checkout path, and stripped symbols keep
/// paths out of the components.
const COMPONENT_PROFILE: &[(&str, &str)] = &[
    ("opt-level", "3"),
    ("debug", "false"),
    ("strip", "\"symbols\""),
    ("debug-assertions", "false"),
    ("overflow-checks", "false"),
    ("lto", "false"),
    ("panic", "\"unwind\""),
    ("incremental", "false"),
    ("codegen-units", "1"),
    ("rpath", "false"),
];

/// [`COMPONENT_PROFILE`] as `--config` values. Cargo config and
/// `CARGO_PROFILE_RELEASE_*` variables override the generated manifest's
/// profile, and the command line overrides both. A per-package override,
/// such as `[profile.release.package."*"]`, still applies to the packages it
/// names; repeating the profile for `"*"` would also apply it to every proc
/// macro and build script.
fn component_profile_config() -> Vec<String> {
    COMPONENT_PROFILE
        .iter()
        .map(|(key, value)| format!("profile.release.{key}={value}"))
        .collect()
}

/// The link inside the generated workspace through which it reaches the
/// checkout. Cargo hashes a path package's location into its symbol names,
/// as a path relative to the workspace when the package is inside it and as
/// an absolute path otherwise, and those names order some functions in a
/// component. Reached through this link, which Cargo keeps as written, the
/// route crate and its path dependencies are inside the generated workspace,
/// so components do not depend on where the checkout is.
const SOURCE_LINK: &str = "source";

/// The local path packages a route build can use, found by following path
/// dependencies from the route crate and its workspace.
struct RouteSources {
    /// The deepest directory holding the Petal root, the route workspace and
    /// every one of `packages`: what the source link points at.
    anchor: PathBuf,
    /// Each local package's directory, resolved through links.
    packages: BTreeSet<PathBuf>,
}

fn route_sources(root: &Path, config: &BuildConfig) -> Result<RouteSources, String> {
    // Cargo applies `..` to a path dependency as written, not through links,
    // and finds a path package's workspace the same way, so this does too.
    let crate_dir = lexical_normalize(&root.join(&config.route_crate.path));
    let workspace = workspace_dir(&crate_dir, &read_toml(&crate_dir.join("Cargo.toml"))?)?;
    let mut anchor = common_ancestor(root, &workspace);
    let mut pending = vec![crate_dir];
    let mut seen = BTreeSet::new();
    let mut packages = BTreeSet::new();
    while let Some(dir) = pending.pop() {
        if !seen.insert(dir.clone()) {
            continue;
        }
        // A missing manifest is left for Cargo to report.
        let Ok(manifest) = read_toml(&dir.join("Cargo.toml")) else {
            continue;
        };
        if manifest.contains_key("package") {
            packages.insert(dir.clone());
        }
        // Cargo reaches the package's workspace through the link as well, to
        // resolve what it inherits.
        let own_workspace = workspace_dir(&dir, &manifest)?;
        anchor = common_ancestor(&common_ancestor(&anchor, &dir), &own_workspace);
        let workspace_manifest = if own_workspace == dir {
            Some(manifest.clone())
        } else {
            read_toml(&own_workspace.join("Cargo.toml")).ok()
        };
        for (section, table) in dependency_tables(&manifest) {
            // Components never build a path package's dev-dependencies, and
            // `[workspace.dependencies]` counts only where a package inherits it.
            if section.ends_with("dev-dependencies]")
                || section.ends_with("dev_dependencies]")
                || section == "[workspace.dependencies]"
            {
                continue;
            }
            for (key, value) in table {
                let path = if value.get("workspace").and_then(toml::Value::as_bool) == Some(true) {
                    workspace_manifest
                        .as_ref()
                        .and_then(|workspace| {
                            workspace
                                .get("workspace")?
                                .get("dependencies")?
                                .get(key)?
                                .get("path")?
                                .as_str()
                        })
                        .map(|path| own_workspace.join(path))
                } else {
                    value
                        .get("path")
                        .and_then(toml::Value::as_str)
                        .map(|path| dir.join(path))
                };
                if let Some(path) = path {
                    pending.push(lexical_normalize(&path));
                }
            }
        }
    }
    Ok(RouteSources { anchor, packages })
}

fn common_ancestor(left: &Path, right: &Path) -> PathBuf {
    left.components()
        .zip(right.components())
        .take_while(|(left, right)| left == right)
        .map(|(component, _)| component)
        .collect()
}

/// Points the source link in `workspace` at `anchor`, and returns the
/// directory through which the generated workspace reaches `anchor`.
#[cfg(unix)]
fn link_checkout(workspace: &Path, anchor: &Path) -> Result<PathBuf, String> {
    let link = workspace.join(SOURCE_LINK);
    let target = PathBuf::from(path_from_member(workspace, anchor)?);
    match fs::read_link(&link) {
        Ok(existing) if existing == target => return Ok(link),
        Ok(_) => fs::remove_file(&link).map_err(display_err("replace", &link))?,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(format!("inspect {}: {error}", link.display())),
    }
    std::os::unix::fs::symlink(&target, &link).map_err(display_err("link", &link))?;
    Ok(link)
}

/// Without Unix links, path packages are reached directly, and components
/// built from two checkouts can differ.
#[cfg(not(unix))]
fn link_checkout(_workspace: &Path, anchor: &Path) -> Result<PathBuf, String> {
    Ok(anchor.to_owned())
}

fn generate_workspace(
    root: &Path,
    workspace: &Path,
    routes: &[Route],
    config: &BuildConfig,
) -> Result<(), String> {
    fs::create_dir_all(workspace).map_err(display_err("create", workspace))?;
    let members = routes
        .iter()
        .map(|r| format!("    \"members/{}\",", r.package))
        .collect::<Vec<_>>()
        .join("\n");
    let profile = COMPONENT_PROFILE
        .iter()
        .map(|(key, value)| format!("{key} = {value}\n"))
        .collect::<String>();
    // Excluding the link keeps Cargo from taking the route crate, which often
    // declares its own workspace, as a member of this one.
    write_if_changed(
        &workspace.join("Cargo.toml"),
        &format!(
            "[workspace]\nresolver = \"2\"\nmembers = [\n{members}\n]\nexclude = [{SOURCE_LINK:?}]\n\n[profile.release]\n{profile}"
        ),
    )?;
    let anchor = route_sources(root, config)?.anchor;
    let reach = link_checkout(workspace, &anchor)?;
    let through = |path: &Path| -> Result<PathBuf, String> {
        let path = lexical_normalize(path);
        let inside = path.strip_prefix(&anchor).map_err(|_| {
            format!(
                "{} is outside {}, which the generated workspace reaches",
                path.display(),
                anchor.display()
            )
        })?;
        Ok(reach.join(inside))
    };
    let members_dir = workspace.join("members");
    fs::create_dir_all(&members_dir).map_err(display_err("create", &members_dir))?;
    for route in routes {
        let dir = members_dir.join(&route.package);
        let mut sdk = route_sdk_dependency(root, config)?;
        if let Some(path) = sdk.get("path").and_then(toml::Value::as_str) {
            let path = path_from_member(&dir, &through(Path::new(path))?)?;
            sdk.as_table_mut()
                .ok_or("SDK dependency must be a table")?
                .insert("path".into(), toml::Value::String(path));
        }
        let sdk = sdk.to_string();
        let route_path = path_from_member(&dir, &through(&root.join(&config.route_crate.path))?)?;
        let mut dependencies = String::new();
        for (name, value) in &config.dependencies {
            dependencies.push_str(&format!("{name} = {value}\n"));
        }
        let manifest = format!(
            "[package]\nname = {:?}\nversion = \"0.1.0\"\nedition = \"2024\"\npublish = false\n\n[lib]\ncrate-type = [\"cdylib\"]\n\n[dependencies]\npetal = {}\n{} = {{ package = {:?}, path = {:?} }}\n{}",
            route.package,
            sdk,
            config.route_crate.alias,
            config.route_crate.package,
            route_path,
            dependencies,
        );
        let params = route_params(&route.path)
            .into_iter()
            .map(|(n, i)| format!("        ({n:?}, {i}),"))
            .collect::<Vec<_>>()
            .join("\n");
        let source_path = path_from_member(&dir.join("src"), &route.source)?;
        let source = format!(
            "#[allow(dead_code)]\npub struct __PetalRouteIdentity;\nimpl petal::RouteIdentity for __PetalRouteIdentity {{\n    const PATH: &'static str = {:?};\n    const CANONICAL_PATH: &'static str = {:?};\n    const PARAMS: &'static [(&'static str, usize)] = &[\n{}\n    ];\n}}\n\n#[allow(unused_imports)]\npub use {}::*;\n\nmod selected_route {{\n    include!({:?});\n}}\n\nuse selected_route::Route;\npetal::bindings::export!(Route);\n",
            route.path, route.canonical, params, config.route_crate.alias, source_path,
        );
        write_if_changed(&dir.join("Cargo.toml"), &manifest)?;
        write_if_changed(&dir.join("src/lib.rs"), &source)?;
    }
    Ok(())
}

fn reuse_route_lockfile(
    root: &Path,
    generated: &Path,
    config: &BuildConfig,
    routes: &[Route],
    locked_sources_only: bool,
) -> Result<(), String> {
    let route_dir = root.join(&config.route_crate.path);
    let manifest = read_toml(&route_dir.join("Cargo.toml"))?;
    let original = workspace_dir(&route_dir, &manifest)?.join("Cargo.lock");
    let seed = fs::read(&original).map_err(|error| {
        format!(
            "read {}: {error}; commit the route workspace lockfile before building components",
            original.display()
        )
    })?;
    fs::write(generated.join("Cargo.lock"), &seed).map_err(display_err("seed", generated))?;
    // Cargo adds the generated wrapper packages from the tested lock. Missing
    // locked sources may be fetched; graph changes are refused before compilation.
    let output = Command::new("cargo")
        .args(["metadata", "--format-version=1"])
        .arg("--color=never")
        .arg("--manifest-path")
        .arg(generated.join("Cargo.toml"))
        .output()
        .map_err(|error| format!("resolve component graph: {error}"))?;
    if !output.status.success() {
        return Err(format!(
            "component dependencies must be provisioned in the route workspace first; refresh and test its Cargo.lock explicitly:\n{}",
            String::from_utf8_lossy(&output.stderr)
        ));
    }
    let old: toml::Table =
        toml::from_str(std::str::from_utf8(&seed).map_err(|error| error.to_string())?)
            .map_err(|error| error.to_string())?;
    let new = read_toml(&generated.join("Cargo.lock"))?;
    let wrappers = routes
        .iter()
        .map(|route| route.package.as_str())
        .collect::<BTreeSet<_>>();
    validate_locked_graph(&old, &new, &wrappers)?;
    if locked_sources_only {
        let metadata: serde_json::Value = serde_json::from_slice(&output.stdout)
            .map_err(|error| format!("parse component graph metadata: {error}"))?;
        let cargo_home = cargo_home().ok_or("cannot find Cargo home")?;
        let packages = route_sources(root, config)?.packages;
        require_locked_sources(
            &old,
            &metadata,
            &cargo_home,
            generated,
            &wrappers,
            &packages,
        )?;
    }
    Ok(())
}

/// With `PETAL_REQUIRE_LOCKED_SOURCES=1`, the component build may use only
/// what the tested lockfile names: a wrapper generated in `generated`; a
/// package the lockfile takes from git or a registry, with that source, read
/// from the copy Cargo fetched into Cargo home; or a local path package the
/// lockfile names, read from one of the route's own path `packages`. The lockfile cannot show a Cargo config `paths` override, which
/// gives a package another path and possibly another version, or a source
/// replacement such as a vendored directory, which keeps the source but reads
/// other files; Cargo's metadata for the generated workspace shows both.
fn require_locked_sources(
    tested: &toml::Table,
    metadata: &serde_json::Value,
    cargo_home: &Path,
    generated: &Path,
    wrappers: &BTreeSet<&str>,
    packages: &BTreeSet<PathBuf>,
) -> Result<(), String> {
    let mut locked = BTreeSet::new();
    let mut local = BTreeSet::new();
    for package in tested
        .get("package")
        .and_then(toml::Value::as_array)
        .into_iter()
        .flatten()
    {
        let (Some(name), Some(version)) = (
            package.get("name").and_then(toml::Value::as_str),
            package.get("version").and_then(toml::Value::as_str),
        ) else {
            continue;
        };
        // Lockfiles percent-encode a branch or tag; metadata does not.
        match package.get("source").and_then(toml::Value::as_str) {
            Some(source) => locked.insert((name, version, percent_decode(source))),
            None => local.insert((name, version)),
        };
    }
    let resolved = |path: &Path| path.canonicalize().unwrap_or_else(|_| path.to_owned());
    let checkouts = resolved(&cargo_home.join("git/checkouts"));
    let downloads = resolved(&cargo_home.join("registry/src"));
    let hashed = |component: &std::path::Component| {
        component
            .as_os_str()
            .to_str()
            .and_then(|name| name.rsplit_once('-'))
            .is_some_and(|(_, hash)| {
                hash.len() == 16 && hash.bytes().all(|b| b.is_ascii_hexdigit())
            })
    };
    // Cargo keeps a registry package in `registry/src/<index>-<hash>/<name>-<version>/`
    // and a git package under `git/checkouts/<repository>-<hash>/<short commit>/`.
    // Cargo home itself is trusted; this keeps a replacement such as a
    // vendored directory from passing for Cargo's own copy.
    let cargo_copy = |manifest: &Path, name: &str, version: &str, source: &str| {
        if let Some((_, commit)) = source
            .strip_prefix("git+")
            .and_then(|source| source.rsplit_once('#'))
        {
            let Ok(rest) = manifest.strip_prefix(&checkouts) else {
                return false;
            };
            let parts = rest.components().collect::<Vec<_>>();
            parts.len() >= 3
                && hashed(&parts[0])
                && parts[1].as_os_str() == std::ffi::OsStr::new(&commit[..commit.len().min(7)])
        } else {
            let Ok(rest) = manifest.strip_prefix(&downloads) else {
                return false;
            };
            let parts = rest.components().collect::<Vec<_>>();
            parts.len() == 3
                && hashed(&parts[0])
                && parts[1].as_os_str() == std::ffi::OsStr::new(&format!("{name}-{version}"))
        }
    };
    let packages = packages
        .iter()
        .map(|dir| resolved(dir))
        .collect::<BTreeSet<_>>();
    let mut refused = Vec::new();
    for package in metadata["packages"].as_array().into_iter().flatten() {
        let (Some(name), Some(version)) = (package["name"].as_str(), package["version"].as_str())
        else {
            continue;
        };
        let manifest = package["manifest_path"]
            .as_str()
            .map(|path| resolved(Path::new(path)));
        let shown = manifest
            .as_ref()
            .map_or("an unknown path".into(), |path| path.display().to_string());
        let wrapper = || {
            wrappers.contains(name)
                && manifest.as_ref()
                    == Some(&resolved(
                        &generated.join("members").join(name).join("Cargo.toml"),
                    ))
        };
        let route_package = || {
            local.contains(&(name, version))
                && manifest
                    .as_ref()
                    .and_then(|path| path.parent())
                    .is_some_and(|dir| packages.contains(dir))
        };
        // Metadata shows a branch or tag as written; only the lockfile encodes it.
        match package["source"].as_str().map(str::to_owned) {
            None if wrapper() || route_package() => {}
            None => refused.push(format!(
                "{name} {version} is read from {shown}, which the tested lockfile does not name"
            )),
            Some(source) if locked.contains(&(name, version, source.clone())) => {
                if !manifest
                    .as_ref()
                    .is_some_and(|path| cargo_copy(path, name, version, &source))
                {
                    refused.push(format!(
                        "{name} {version} is read from {shown}, not Cargo's copy of {source}"
                    ));
                }
            }
            Some(source) => refused.push(format!(
                "{name} {version} from {source} is not in the tested lockfile"
            )),
        }
    }
    if refused.is_empty() {
        return Ok(());
    }
    Err(format!(
        "PETAL_REQUIRE_LOCKED_SOURCES=1 refuses packages that Cargo config moves away from the tested lockfile, through a [patch], a paths override or a source replacement:\n{}",
        refused
            .iter()
            .map(|line| format!("  - {line}"))
            .collect::<Vec<_>>()
            .join("\n")
    ))
}

fn validate_locked_graph(
    original: &toml::Table,
    generated: &toml::Table,
    wrappers: &BTreeSet<&str>,
) -> Result<(), String> {
    let packages = |lock: &toml::Table| {
        lock.get("package")
            .and_then(toml::Value::as_array)
            .cloned()
            .unwrap_or_default()
    };
    let tested = packages(original);
    let resolved = packages(generated);
    let identity = |package: &toml::Value| {
        (
            package.get("name").cloned(),
            package.get("version").cloned(),
            package.get("source").cloned(),
        )
    };
    for package in &resolved {
        let name = package
            .get("name")
            .and_then(toml::Value::as_str)
            .ok_or("lock package missing name")?;
        if wrappers.contains(name) && package.get("source").is_none() {
            continue;
        }
        let original = tested.iter().find(|old| identity(old) == identity(package));
        if original.is_none_or(|old| old.get("checksum") != package.get("checksum")) {
            return Err(format!(
                "component dependency {name} differs from the tested route Cargo.lock; declare wrapper dependencies in the route workspace, then explicitly refresh and test its lockfile"
            ));
        }
        let dependencies = |package: &toml::Value| {
            package
                .get("dependencies")
                .and_then(toml::Value::as_array)
                .cloned()
                .unwrap_or_default()
        };
        let old_dependencies = dependencies(original.unwrap())
            .iter()
            .map(|dependency| lock_dependency_identity(dependency, &tested))
            .collect::<Result<Vec<_>, _>>()?;
        for dependency in dependencies(package) {
            if !old_dependencies.contains(&lock_dependency_identity(&dependency, &resolved)?) {
                return Err(format!(
                    "component dependency edges for {name} differ from the tested route Cargo.lock; refresh and test the route workspace explicitly"
                ));
            }
        }
    }
    Ok(())
}

fn lock_dependency_identity(
    dependency: &toml::Value,
    packages: &[toml::Value],
) -> Result<(String, String, Option<String>), String> {
    let text = dependency.as_str().ok_or("invalid lockfile dependency")?;
    let mut parts = text.split_whitespace();
    let name = parts.next().ok_or("empty lockfile dependency")?;
    let version = parts.next();
    let source = parts.next().map(|source| source.trim_matches(['(', ')']));
    let matches = packages
        .iter()
        .filter(|package| {
            package.get("name").and_then(toml::Value::as_str) == Some(name)
                && version.is_none_or(|version| {
                    package.get("version").and_then(toml::Value::as_str) == Some(version)
                })
                && source.is_none_or(|source| {
                    package
                        .get("source")
                        .and_then(toml::Value::as_str)
                        .map(|locked| locked.split('#').next().unwrap_or(locked))
                        == Some(source)
                })
        })
        .collect::<Vec<_>>();
    if matches.len() != 1 {
        return Err(format!("ambiguous lockfile dependency {text}"));
    }
    let package = matches[0];
    Ok((
        name.into(),
        package
            .get("version")
            .and_then(toml::Value::as_str)
            .ok_or("missing locked version")?
            .into(),
        package
            .get("source")
            .and_then(toml::Value::as_str)
            .map(str::to_owned),
    ))
}

fn build_workspace(
    root: &Path,
    workspace: &Path,
    routes: &[Route],
    jobs: Option<usize>,
) -> Result<BTreeMap<String, PathBuf>, String> {
    let mut cmd = Command::new("cargo");
    cmd.args([
        "build",
        "--workspace",
        "--target",
        "wasm32-unknown-unknown",
        "--release",
        "--locked",
        "--message-format=json-render-diagnostics",
    ])
    .arg("--manifest-path")
    .arg(workspace.join("Cargo.toml"))
    .arg("--target-dir")
    .arg(workspace.parent().unwrap().join("target"))
    .stdout(Stdio::piped())
    .stderr(Stdio::inherit());
    for setting in component_profile_config() {
        cmd.arg("--config").arg(setting);
    }
    // Cargo applies `CARGO_INCREMENTAL`, then `build.incremental`, over every
    // profile, and an incremental build partitions code differently.
    cmd.env("CARGO_INCREMENTAL", "0");
    let flags = component_rustflags(root)?;
    if std::env::var_os("CARGO_ENCODED_RUSTFLAGS").is_some()
        || std::env::var_os("RUSTFLAGS").is_some()
    {
        cmd.env("CARGO_ENCODED_RUSTFLAGS", flags);
    } else {
        // Add to configured build flags. Do not replace a developer's
        // target-specific flags with an environment override.
        let flags = flags.split('\x1f').collect::<Vec<_>>();
        let environment = rustflags_variable("CARGO_BUILD_RUSTFLAGS")?;
        match appended_build_rustflags(environment.as_deref(), &flags) {
            // Cargo appends this variable to `build.rustflags` wherever its
            // config sets them, as a string or an array.
            Some(value) => {
                cmd.env("CARGO_BUILD_RUSTFLAGS", value);
            }
            // Cargo splits the variable on whitespace, so a path containing
            // whitespace goes in an array.
            None => {
                let setting = rustflags_array_setting(
                    Command::new("cargo"),
                    &workspace.join("Cargo.toml"),
                    &flags,
                )?;
                cmd.arg("--config").arg(setting);
            }
        }
    }
    if let Some(jobs) = jobs {
        cmd.arg("--jobs").arg(jobs.to_string());
    }
    let mut child = cmd.spawn().map_err(|e| format!("run cargo build: {e}"))?;
    let stdout = child.stdout.take().ok_or("cargo stdout unavailable")?;
    let wanted = routes
        .iter()
        .map(|r| r.package.as_str())
        .collect::<BTreeSet<_>>();
    let mut artifacts: BTreeMap<String, Vec<PathBuf>> = BTreeMap::new();
    for line in BufReader::new(stdout).lines() {
        let line = line.map_err(|e| e.to_string())?;
        let Ok(value) = serde_json::from_str::<serde_json::Value>(&line) else {
            continue;
        };
        if value["reason"] != "compiler-artifact"
            || !value["target"]["kind"]
                .as_array()
                .is_some_and(|a| a.iter().any(|k| k == "cdylib"))
        {
            continue;
        }
        let Some(name) = value["target"]["name"].as_str() else {
            continue;
        };
        let package = name.replace('_', "-");
        if !wanted.contains(package.as_str()) {
            continue;
        }
        for filename in value["filenames"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|v| v.as_str())
        {
            if filename.ends_with(".wasm") {
                artifacts
                    .entry(package.clone())
                    .or_default()
                    .push(filename.into());
            }
        }
    }
    let status = child.wait().map_err(|e| e.to_string())?;
    if !status.success() {
        return Err(format!("cargo workspace build failed with {status}"));
    }
    let mut exact = BTreeMap::new();
    for route in routes {
        let files = artifacts.remove(&route.package).unwrap_or_default();
        if files.len() != 1 {
            return Err(format!(
                "{} produced {} core WASMs",
                route.package,
                files.len()
            ));
        }
        exact.insert(route.package.clone(), files[0].clone());
    }
    Ok(exact)
}

/// `CARGO_BUILD_RUSTFLAGS` with `flags` after the developer's own value, or
/// `None` when a flag contains whitespace, which Cargo would split it on.
fn appended_build_rustflags(developer: Option<&str>, flags: &[&str]) -> Option<String> {
    if flags.iter().any(|flag| flag.contains(char::is_whitespace)) {
        return None;
    }
    Some(
        developer
            .map(str::trim)
            .filter(|developer| !developer.is_empty())
            .into_iter()
            .chain(flags.iter().copied())
            .collect::<Vec<_>>()
            .join(" "),
    )
}

/// The `--config` setting that gives Cargo `flags` as a `build.rustflags`
/// array, which keeps a path containing whitespace in one flag. Cargo appends
/// it to an array that Cargo config sets, but stops at a string with only
/// "expected string, but found array", so ask `cargo` first and say why.
fn rustflags_array_setting(
    mut cargo: Command,
    manifest: &Path,
    flags: &[&str],
) -> Result<String, String> {
    let setting = format!(
        "build.rustflags={}",
        serde_json::to_string(flags).map_err(|error| error.to_string())?
    );
    let output = cargo
        .args([
            "metadata",
            "--format-version",
            "1",
            "--no-deps",
            "--offline",
        ])
        .arg("--manifest-path")
        .arg(manifest)
        .arg("--config")
        .arg(&setting)
        .stdin(Stdio::null())
        .output()
        .map_err(|error| format!("run cargo metadata: {error}"))?;
    // Any other failure is the build's to report.
    if !output.status.success()
        && String::from_utf8_lossy(&output.stderr).contains("expected string, but found array")
    {
        return Err("the checkout, Cargo home or toolchain path contains whitespace, so the builder passes its path remapping as a build.rustflags array, which Cargo cannot merge into the string build.rustflags that Cargo config sets. Set build.rustflags as an array there, such as rustflags = [\"--cfg\", \"name\"], or build from paths without whitespace".into());
    }
    Ok(setting)
}

fn rustflags_variable(name: &str) -> Result<Option<String>, String> {
    utf8_rustflags(name, std::env::var_os(name))
}

/// A rustflags variable's value. Cargo ignores one that is not UTF-8, which
/// would silently drop the developer's flags or the builder's, so refuse it.
fn utf8_rustflags(name: &str, value: Option<std::ffi::OsString>) -> Result<Option<String>, String> {
    value
        .map(|value| {
            value
                .into_string()
                .map_err(|_| format!("{name} must be UTF-8"))
        })
        .transpose()
}

fn cargo_home() -> Option<PathBuf> {
    std::env::var_os("CARGO_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".cargo")))
}

/// The toolchain's rust-src directory and the `/rustc/<commit>` path rustc
/// embeds for standard library sources when rust-src is not installed.
fn toolchain_source() -> Result<Option<(PathBuf, String)>, String> {
    let rustc = std::env::var_os("RUSTC").unwrap_or_else(|| "rustc".into());
    let print = |args: &[&str]| -> Result<String, String> {
        let output = Command::new(&rustc)
            .args(args)
            .output()
            .map_err(|error| format!("run rustc {}: {error}", args.join(" ")))?;
        if !output.status.success() {
            return Err(format!(
                "rustc {} failed with {}",
                args.join(" "),
                output.status
            ));
        }
        String::from_utf8(output.stdout)
            .map_err(|_| format!("rustc {} printed non-UTF-8 output", args.join(" ")))
    };
    let version = print(&["-vV"])?;
    let Some(commit) = version
        .lines()
        .find_map(|line| line.strip_prefix("commit-hash: "))
        .filter(|commit| is_full_commit(commit))
    else {
        return Ok(None);
    };
    let sysroot = print(&["--print", "sysroot"])?;
    Ok(Some((
        PathBuf::from(sysroot.trim()).join("lib/rustlib/src/rust"),
        format!("/rustc/{commit}"),
    )))
}

/// The builder's flags after the developer's `CARGO_ENCODED_RUSTFLAGS`, or
/// `RUSTFLAGS` when that is unset, as Cargo would choose between them.
fn component_rustflags(root: &Path) -> Result<String, String> {
    let mut flags =
        std::ffi::OsString::from(match rustflags_variable("CARGO_ENCODED_RUSTFLAGS")? {
            Some(encoded) => encoded,
            None => rustflags_variable("RUSTFLAGS")?
                .unwrap_or_default()
                .split_whitespace()
                .collect::<Vec<_>>()
                .join("\x1f"),
        });
    let mut remap = |path: &Path, stable: &str| {
        if !flags.is_empty() {
            flags.push("\x1f");
        }
        flags.push("--remap-path-prefix=");
        flags.push(path);
        flags.push("=");
        flags.push(stable);
    };
    remap(root, "/petal");
    if let Some(cargo_home) = cargo_home() {
        remap(&cargo_home, "/cargo");
    }
    // With rust-src installed, rustc embeds standard library paths inside the
    // toolchain, which names its directory and the user's home. Map them back
    // to the path rustc embeds without rust-src.
    if let Some((source, embedded)) = toolchain_source()? {
        remap(&source, &embedded);
    }
    // Cargo ignores a rustflags variable that is not UTF-8.
    flags.into_string().map_err(|_| {
        "component rustflags must be UTF-8, but the checkout, Cargo home or toolchain path is not"
            .into()
    })
}

fn discover_routes(root: &Path) -> Result<Vec<Route>, String> {
    let mut sources = Vec::new();
    discover_at(root, root, &mut sources)?;
    sources.sort();
    let mut routes = Vec::new();
    let mut packages = BTreeSet::new();
    let mut paths = BTreeSet::new();
    for source in sources {
        let path = route_path(root, &source)?;
        let canonical = canonical_route_path(&path);
        let package = package_name(&path);
        if !paths.insert(path.clone()) || !packages.insert(package.clone()) {
            return Err(format!("route identity collision for {path}"));
        }
        routes.push(Route {
            path,
            canonical,
            source,
            package,
        });
    }
    Ok(routes)
}

fn discover_at(root: &Path, dir: &Path, out: &mut Vec<PathBuf>) -> Result<(), String> {
    for entry in fs::read_dir(dir).map_err(display_err("read", dir))? {
        let path = entry.map_err(|e| e.to_string())?.path();
        if path.is_dir() {
            discover_at(root, &path, out)?;
        } else if path.extension() == Some(OsStr::new("rs")) {
            if path.file_name() == Some(OsStr::new("$list.rs")) {
                return Err(format!("{} is unsupported; use $index.rs", path.display()));
            }
            out.push(path);
        }
    }
    let _ = root;
    Ok(())
}

fn route_path(root: &Path, source: &Path) -> Result<String, String> {
    let mut path = source
        .strip_prefix(root)
        .map_err(|e| e.to_string())?
        .to_string_lossy()
        .replace('\\', "/");
    path.truncate(path.len() - 3);
    Ok(path)
}
fn canonical_route_path(path: &str) -> String {
    if path == "$index" {
        "".into()
    } else {
        path.strip_suffix("/$index").unwrap_or(path).into()
    }
}
fn route_params(path: &str) -> Vec<(&str, usize)> {
    path.split('/')
        .enumerate()
        .filter_map(|(i, s)| {
            s.strip_prefix('[')
                .and_then(|s| s.strip_suffix(']'))
                .map(|n| (n, i))
        })
        .collect()
}
fn package_name(path: &str) -> String {
    let readable = path
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() {
                c.to_ascii_lowercase()
            } else {
                '-'
            }
        })
        .collect::<String>();
    let readable = readable
        .split('-')
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>()
        .join("-");
    let hash = blake3::hash(path.as_bytes()).to_hex();
    format!(
        "petal-route-{}-{}",
        if readable.is_empty() {
            "root"
        } else {
            &readable
        },
        &hash[..10]
    )
}

fn write_if_changed(path: &Path, content: &str) -> Result<(), String> {
    if fs::read_to_string(path).ok().as_deref() == Some(content) {
        return Ok(());
    }
    fs::create_dir_all(path.parent().unwrap())
        .map_err(display_err("create", path.parent().unwrap()))?;
    fs::write(path, content).map_err(display_err("write", path))
}

fn check_route_caps(route: &Route, artifact: &Path) -> Result<(), String> {
    let required = required_caps(&route.source)?;
    let bytes = fs::read(artifact).map_err(display_err("read", artifact))?;
    let imported = imported_caps(&bytes)?;
    let missing = required.difference(&imported).copied().collect::<Vec<_>>();
    if missing.is_empty() {
        Ok(())
    } else {
        Err(format!(
            "route {} requires absent imports: {}",
            route.path,
            missing.join(", ")
        ))
    }
}

/// Report whether `body` calls `accessor`, ignoring mentions in prose and
/// longer identifiers that merely start with the same name.
fn calls(body: &str, accessor: &str) -> bool {
    body.match_indices(accessor).any(|(start, _)| {
        let follows_identifier = body[..start]
            .chars()
            .next_back()
            .is_some_and(|char| char.is_alphanumeric() || char == '_');
        let call_site = body[start + accessor.len()..].trim_start().starts_with('(');
        !follows_identifier && call_site
    })
}

fn check_shared_route_architecture(root: &Path, config: &BuildConfig) -> Result<(), String> {
    let source_root = root.join(&config.route_crate.path).join("src");
    let mut sources = Vec::new();
    discover_at(&source_root, &source_root, &mut sources)?;
    let mut violations = Vec::new();
    for source in sources {
        let body = fs::read_to_string(&source).map_err(display_err("read", &source))?;
        for accessor in ["current_route_path", "current_route_canonical_path"] {
            if calls(&body, accessor) {
                violations.push(format!(
                    "{} uses forbidden route-identity accessor {accessor}; keep dispatch behavior in route files",
                    source.display()
                ));
            }
        }
    }
    if violations.is_empty() {
        Ok(())
    } else {
        Err(violations.join("\n"))
    }
}

/// Report wallet-scoped routes Bloom will refuse before the Petal runs.
///
/// A route carrying `[wallet]` only reaches a guest when `[index]` follows it
/// immediately: Bloom's router denies every other shape
/// (`bloom-petals/src/router.rs`, the `(Some(wallet), Some(index))` adjacency
/// check and the `_ => PermissionDenied` fallthrough). The exception is the
/// directory listing at `…/[wallet]/$index`, which Bloom answers from its own
/// account projection — legal provided the Petal does declare the account
/// routes beneath it.
///
/// Warnings rather than errors for now: several shipped Petals are in exactly
/// this state, and failing their builds is a separate decision from reporting
/// the breakage. This becomes an error once they are migrated.
fn warn_route_shapes(routes: &[Route]) {
    for violation in route_shape_violations(routes) {
        eprintln!("warning: {violation}");
    }
}

/// The name of a route capture, parsed the way Bloom parses it: `[wallet]`
/// and `[wallet].json` both name `wallet`. Bloom binds the suffixed form too,
/// so a lint matching only the exact segment misses routes Bloom refuses.
fn capture_name(segment: &str) -> Option<&str> {
    let rest = segment.strip_prefix('[')?;
    rest.find(']').map(|end| &rest[..end])
}

fn route_shape_violations(routes: &[Route]) -> Vec<String> {
    let wallet = bloom_petal_contract::WALLET_CAPTURE;
    let index = bloom_petal_contract::ACCOUNT_INDEX_CAPTURE;
    let has_account_routes = |prefix: &str| {
        let nested = format!("{prefix}/{index}");
        routes
            .iter()
            .any(|route| route.path == nested || route.path.starts_with(&format!("{nested}/")))
    };
    let mut violations = Vec::new();
    for route in routes {
        let segments = route.path.split('/').collect::<Vec<_>>();
        for (position, segment) in segments.iter().enumerate() {
            match capture_name(segment) {
                // Bloom compares the exact segments `[wallet]` and `[index]`
                // for adjacency, so a suffixed capture can never satisfy it,
                // even though Bloom still binds the parameter from it.
                Some("wallet") | Some("index") if *segment != wallet && *segment != index => {
                    violations.push(format!(
                        "route {} captures {segment}; Bloom only grants wallet authority to the exact segments {wallet}/{index}, so it refuses this route before the Petal runs",
                        route.path
                    ));
                }
                Some("wallet") => {
                    let prefix = segments[..=position].join("/");
                    match segments.get(position + 1) {
                        Some(next) if *next == index => {}
                        // The host-owned account listing. Only meaningful when
                        // the account routes it points at actually exist.
                        Some(next) if *next == "$index" && has_account_routes(&prefix) => {}
                        Some(next) if capture_name(next) == Some("index") => {} // reported at that segment
                        _ => violations.push(format!(
                            "route {} declares {wallet} without an adjacent {index}; Bloom refuses it before the Petal runs",
                            route.path
                        )),
                    }
                }
                Some("index")
                    if position
                        .checked_sub(1)
                        .and_then(|previous| segments.get(previous))
                        != Some(&wallet) =>
                {
                    violations.push(format!(
                        "route {} declares {index} without a preceding {wallet}",
                        route.path
                    ));
                }
                _ => {}
            }
        }
    }
    violations
}

fn required_caps(source: &Path) -> Result<BTreeSet<&'static str>, String> {
    let source = fs::read_to_string(source).map_err(display_err("read", source))?;
    if let Some(start) = source.find(".caps(&[") {
        let rest = &source[start + 8..];
        let end = rest.find("])").ok_or("unterminated caps override")?;
        return Ok(ALL_CAPS
            .iter()
            .copied()
            .filter(|cap| rest[..end].contains(cap))
            .collect());
    }
    let caps: &[&str] = if source.contains("store_dir_spec()") {
        &["bloom:store", "bloom:vfs.read"]
    } else if source.contains("account_read_spec()") {
        &["bloom:http", "bloom:store", "bloom:vfs.read"]
    } else if source.contains("wallet_http_read_spec(") {
        &["bloom:http", "bloom:vfs.read"]
    } else if source.contains("http_dir_spec()") || source.contains("http_read_spec(") {
        &["bloom:http"]
    } else if source.contains("store_read_spec()") {
        &["bloom:store"]
    } else if source.contains("chain_read_spec()")
        || source.contains("write_spec()")
        || source.contains("signing_write_spec(")
    {
        ALL_CAPS
    } else {
        &[]
    };
    Ok(caps.iter().copied().collect())
}

fn imported_caps(wasm: &[u8]) -> Result<BTreeSet<&'static str>, String> {
    let mut caps = BTreeSet::new();
    let mut depth = 0usize;
    for payload in Parser::new(0).parse_all(wasm) {
        let payload = payload.map_err(|error| format!("parse component imports: {error}"))?;
        let current_depth = depth;
        match payload {
            Payload::ComponentImportSection(reader) if current_depth == 0 => {
                for import in reader {
                    let import =
                        import.map_err(|error| format!("parse component import: {error}"))?;
                    let name = import.name.0;
                    if import.ty.kind() == ComponentExternalKind::Type
                        && matches!(
                            name,
                            "ctx" | "entry-kind" | "entry" | "route-meta" | "route-error"
                        )
                    {
                        continue;
                    }
                    if import.ty.kind() != ComponentExternalKind::Instance {
                        return Err(format!(
                            "component imports unsupported non-interface host item {name:?}"
                        ));
                    }
                    let import_caps = bloom_petal_contract::capabilities_for_import(name)
                        .ok_or_else(|| {
                            format!("component imports unsupported host item {name:?}")
                        })?;
                    caps.extend(import_caps.iter().copied());
                }
            }
            Payload::ModuleSection { .. } | Payload::ComponentSection { .. } => depth += 1,
            Payload::End(_) => depth = depth.saturating_sub(1),
            _ => {}
        }
    }
    Ok(caps)
}

/// The components under `root`, as Bloom would install them. A link is
/// refused rather than followed: a link back up the tree, or to `/`, would
/// otherwise be walked before packaging refuses it.
fn discover_wasm_paths(root: &Path) -> Result<BTreeSet<String>, String> {
    fn refuse_link(path: &Path) -> String {
        format!(
            "{} is a link; a Petal runtime tree holds only files and directories",
            path.display()
        )
    }
    fn walk(root: &Path, dir: &Path, out: &mut BTreeSet<String>) -> Result<(), String> {
        for entry in fs::read_dir(dir).map_err(display_err("read", dir))? {
            let entry = entry.map_err(|e| e.to_string())?;
            let path = entry.path();
            let file_type = entry.file_type().map_err(display_err("inspect", &path))?;
            if file_type.is_symlink() {
                return Err(refuse_link(&path));
            }
            if file_type.is_dir() {
                walk(root, &path, out)?;
            // Bloom installs every file whose name ends in `.wasm` as a route,
            // including one named just `.wasm`, which has no extension.
            } else if path
                .file_name()
                .is_some_and(|name| name.as_encoded_bytes().ends_with(b".wasm"))
            {
                out.insert(
                    path.strip_prefix(root)
                        .unwrap()
                        .to_string_lossy()
                        .replace('\\', "/"),
                );
            }
        }
        Ok(())
    }
    if fs::symlink_metadata(root)
        .map_err(display_err("inspect", root))?
        .file_type()
        .is_symlink()
    {
        return Err(refuse_link(root));
    }
    let mut out = BTreeSet::new();
    walk(root, root, &mut out)?;
    Ok(out)
}

fn require_tool(tool: &str) -> Result<(), String> {
    command(
        Command::new(tool)
            .arg("--version")
            .stdout(Stdio::null())
            .stderr(Stdio::null()),
    )
}
fn tool_version(tool: &str) -> Result<String, String> {
    let output = Command::new(tool)
        .arg("--version")
        .output()
        .map_err(|error| format!("run {tool} --version: {error}"))?;
    if !output.status.success() {
        return Err(format!("{tool} --version failed with {}", output.status));
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}
fn command(command: &mut Command) -> Result<(), String> {
    let status = command.status().map_err(|e| format!("run command: {e}"))?;
    if status.success() {
        Ok(())
    } else {
        Err(format!("command failed with {status}"))
    }
}
fn display_err<'a>(action: &'a str, path: &'a Path) -> impl FnOnce(std::io::Error) -> String + 'a {
    move |e| format!("{action} {}: {e}", path.display())
}

fn validate_name(name: &str) -> Result<(), String> {
    if name.is_empty()
        || !name
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
    {
        return Err("Petal name must use only ASCII letters, digits, '-' and '_'".into());
    }
    Ok(())
}

fn validate_alias(alias: &str) -> Result<(), String> {
    if alias.is_empty()
        || !alias
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
        || alias.as_bytes()[0].is_ascii_digit()
    {
        return Err("route_crate.alias must be a Rust identifier".into());
    }
    Ok(())
}

fn validate_relative(path: &Path, field: &str) -> Result<(), String> {
    if path.as_os_str().is_empty()
        || path.is_absolute()
        || path.components().any(|component| {
            matches!(
                component,
                std::path::Component::ParentDir | std::path::Component::CurDir
            )
        })
    {
        return Err(format!("{field} must be a normalized relative path"));
    }
    Ok(())
}

fn validate_sdk(sdk: &SdkDependency) -> Result<(), String> {
    let selected = usize::from(sdk.version.is_some())
        + usize::from(sdk.path.is_some())
        + usize::from(sdk.git.is_some());
    if selected != 1 {
        return Err("sdk must specify exactly one of version, path, or git".into());
    }
    if sdk.rev.is_some() && sdk.git.is_none() {
        return Err("sdk.rev requires sdk.git".into());
    }
    // An exact pin, so every build and every test resolves the same SDK.
    if sdk.git.is_some() && !sdk.rev.as_deref().is_some_and(is_full_commit) {
        return Err("sdk.git requires sdk.rev, a full 40-character commit".into());
    }
    if let Some(version) = sdk
        .version
        .as_ref()
        .filter(|version| !is_exact_version(version))
    {
        return Err(format!(
            "sdk.version must be an exact requirement such as \"=0.1.0\", not {version:?}"
        ));
    }
    if let Some(path) = &sdk.path {
        validate_relative(path, "sdk.path")?;
    }
    Ok(())
}

/// Resolve the route's actual SDK dependency, preserving feature selection.
/// `[sdk]` is only an optional migration assertion; it never drives compilation.
fn route_sdk_dependency(root: &Path, config: &BuildConfig) -> Result<toml::Value, String> {
    let crate_dir = root.join(&config.route_crate.path);
    let manifest = read_toml(&crate_dir.join("Cargo.toml"))?;
    let workspace = workspace_dir(&crate_dir, &manifest)?;
    let workspace_manifest = read_toml(&workspace.join("Cargo.toml"))?;
    let mut matches = Vec::new();
    for (key, value) in manifest
        .get("dependencies")
        .and_then(toml::Value::as_table)
        .into_iter()
        .flatten()
    {
        let inherited = value.get("workspace").and_then(toml::Value::as_bool) == Some(true);
        let dependency = if inherited {
            workspace_manifest
                .get("workspace")
                .and_then(|v| v.get("dependencies"))
                .and_then(|v| v.get(key))
                .ok_or_else(|| format!("missing workspace dependency {key}"))?
        } else {
            value
        };
        if dependency_package(key, dependency) != config.sdk.package {
            continue;
        }
        let mut table = match dependency {
            toml::Value::String(version) => {
                toml::Table::from_iter([("version".into(), toml::Value::String(version.clone()))])
            }
            toml::Value::Table(table) => table.clone(),
            _ => return Err("SDK dependency must be a version or table".into()),
        };
        // An inheriting entry may add features, drop default features or make
        // the dependency optional; Cargo allows `optional` only there.
        if inherited {
            for field in ["features", "default-features", "optional"] {
                if let Some(local) = value.get(field) {
                    if field == "features" {
                        let existing = table
                            .entry(field)
                            .or_insert_with(|| toml::Value::Array(Vec::new()));
                        existing
                            .as_array_mut()
                            .ok_or("SDK features must be an array")?
                            .extend(
                                local
                                    .as_array()
                                    .ok_or("SDK features must be an array")?
                                    .iter()
                                    .cloned(),
                            );
                    } else {
                        table.insert(field.into(), local.clone());
                    }
                }
            }
        }
        if table.get("optional").and_then(toml::Value::as_bool) == Some(true) {
            return Err("route SDK dependency must not be optional".into());
        }
        if let Some(path) = table.get("path").and_then(toml::Value::as_str) {
            let base = if inherited { &workspace } else { &crate_dir };
            // Cargo applies `..` to the path as written, not through links.
            resolve_sdk_path(base, Path::new(path))?;
            let path = lexical_normalize(&base.join(path));
            table.insert(
                "path".into(),
                toml::Value::String(path.to_string_lossy().into_owned()),
            );
        }
        table.insert(
            "package".into(),
            toml::Value::String(config.sdk.package.clone()),
        );
        matches.push(toml::Value::Table(table));
    }
    if matches.len() != 1 {
        return Err("route crate must declare exactly one bloom-petal-sdk dependency".into());
    }
    Ok(matches.remove(0))
}

/// The crates.io index, in the forms Cargo writes to a lockfile.
const CRATES_IO_SOURCES: [&str; 2] = [
    "registry+https://github.com/rust-lang/crates.io-index",
    "sparse+https://index.crates.io/",
];

/// One dependency source, normalized so that equal pins compare equal wherever
/// they are written: paths are resolved, git URLs are canonicalized the way
/// Cargo does, and version requirements ignore whitespace.
#[derive(Clone, Debug, PartialEq, Eq)]
enum SdkSource {
    Version {
        requirement: String,
        /// An alternate registry, which `[sdk]` cannot express.
        registry: Option<String>,
    },
    Path(PathBuf),
    Git {
        url: String,
        rev: Option<String>,
        /// A `branch` or `tag` reference, which `[sdk]` cannot express.
        other_ref: Option<String>,
    },
}

impl std::fmt::Display for SdkSource {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Version {
                requirement,
                registry,
            } => {
                write!(f, "version {requirement}")?;
                if let Some(registry) = registry {
                    write!(f, " from registry {registry}")?;
                }
                Ok(())
            }
            Self::Path(path) => write!(f, "path {}", path.display()),
            Self::Git {
                url,
                rev,
                other_ref,
            } => {
                write!(f, "git {url}")?;
                if let Some(rev) = rev {
                    write!(f, " rev {rev}")?;
                }
                if let Some(other) = other_ref {
                    write!(f, " {other}")?;
                }
                Ok(())
            }
        }
    }
}

/// Canonicalize a git URL as Cargo does for source identity: drop one trailing
/// `/` and one `.git`, and treat GitHub paths as case-insensitive and https.
fn normalize_git_url(url: &str) -> String {
    let url = url.trim();
    let url = url.strip_suffix('/').unwrap_or(url);
    let url = url.strip_suffix(".git").unwrap_or(url);
    let lower = url.to_ascii_lowercase();
    for scheme in ["https://github.com/", "http://github.com/"] {
        if let Some(path) = lower.strip_prefix(scheme) {
            return format!("https://github.com/{path}");
        }
    }
    url.to_owned()
}

fn normalize_version(requirement: &str) -> String {
    requirement.split_whitespace().collect()
}

/// `=MAJOR.MINOR.PATCH`, optionally with a pre-release or build suffix.
fn is_exact_version(requirement: &str) -> bool {
    let Some(version) = normalize_version(requirement)
        .strip_prefix('=')
        .map(str::to_owned)
    else {
        return false;
    };
    let core = version.split(['-', '+']).next().unwrap_or("");
    let parts = core.split('.').collect::<Vec<_>>();
    parts.len() == 3
        && parts
            .iter()
            .all(|part| !part.is_empty() && part.bytes().all(|byte| byte.is_ascii_digit()))
        && version
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'-' | b'+'))
}

fn is_full_commit(rev: &str) -> bool {
    rev.len() == 40 && rev.bytes().all(|byte| byte.is_ascii_hexdigit())
}

fn percent_decode(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        let hex = |byte: u8| char::from(byte).to_digit(16);
        if bytes[index] == b'%' && index + 2 < bytes.len() {
            if let (Some(high), Some(low)) = (hex(bytes[index + 1]), hex(bytes[index + 2])) {
                out.push((high * 16 + low) as u8);
                index += 3;
                continue;
            }
        }
        out.push(bytes[index]);
        index += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn resolve_sdk_path(base: &Path, path: &Path) -> Result<PathBuf, String> {
    let joined = base.join(path);
    joined
        .canonicalize()
        .map_err(|error| format!("resolve SDK path {}: {error}", joined.display()))
}

fn configured_sdk_source(root: &Path, sdk: &SdkDependency) -> Result<SdkSource, String> {
    if let Some(version) = &sdk.version {
        return Ok(SdkSource::Version {
            requirement: normalize_version(version),
            registry: None,
        });
    }
    if let Some(path) = &sdk.path {
        return Ok(SdkSource::Path(resolve_sdk_path(root, path)?));
    }
    match &sdk.git {
        Some(git) => Ok(SdkSource::Git {
            url: normalize_git_url(git),
            rev: sdk.rev.clone(),
            other_ref: None,
        }),
        None => Err("sdk must specify exactly one of version, path, or git".into()),
    }
}

/// The source a dependency entry names. `base` is the directory its paths are
/// relative to.
fn dependency_source(base: &Path, key: &str, value: &toml::Value) -> Result<SdkSource, String> {
    let Some(table) = value.as_table() else {
        return Ok(SdkSource::Version {
            requirement: normalize_version(value.as_str().unwrap_or_default()),
            registry: None,
        });
    };
    let text = |field: &str| table.get(field).and_then(toml::Value::as_str);
    if let Some(git) = text("git") {
        let other_ref = text("branch")
            .map(|branch| format!("branch {branch}"))
            .or_else(|| text("tag").map(|tag| format!("tag {tag}")));
        return Ok(SdkSource::Git {
            url: normalize_git_url(git),
            rev: text("rev").map(str::to_owned),
            other_ref,
        });
    }
    if let Some(path) = text("path") {
        return Ok(SdkSource::Path(resolve_sdk_path(base, Path::new(path))?));
    }
    match text("version") {
        Some(version) => Ok(SdkSource::Version {
            requirement: normalize_version(version),
            registry: text("registry")
                .or_else(|| text("registry-index"))
                .map(str::to_owned),
        }),
        None => Err(format!(
            "dependency {key} has no version, path, or git source"
        )),
    }
}

/// The package a dependency entry resolves to: its `package` key, or its name.
fn dependency_package<'a>(key: &'a str, value: &'a toml::Value) -> &'a str {
    value
        .get("package")
        .and_then(toml::Value::as_str)
        .unwrap_or(key)
}

/// Every dependency table in a manifest, with a label for messages:
/// `[dependencies]`, `[dev-dependencies]`, `[build-dependencies]` (and their
/// older underscore spellings), their `[target.<cfg>.…]` forms, and
/// `[workspace.dependencies]`.
fn dependency_tables(manifest: &toml::Table) -> Vec<(String, &toml::Table)> {
    const KINDS: [&str; 5] = [
        "dependencies",
        "dev-dependencies",
        "dev_dependencies",
        "build-dependencies",
        "build_dependencies",
    ];
    let mut tables = Vec::new();
    for kind in KINDS {
        if let Some(table) = manifest.get(kind).and_then(toml::Value::as_table) {
            tables.push((format!("[{kind}]"), table));
        }
    }
    if let Some(targets) = manifest.get("target").and_then(toml::Value::as_table) {
        for (target, value) in targets {
            for kind in KINDS {
                if let Some(table) = value.get(kind).and_then(toml::Value::as_table) {
                    tables.push((format!("[target.{target}.{kind}]"), table));
                }
            }
        }
    }
    if let Some(table) = manifest
        .get("workspace")
        .and_then(|workspace| workspace.get("dependencies"))
        .and_then(toml::Value::as_table)
    {
        tables.push(("[workspace.dependencies]".into(), table));
    }
    tables
}

/// Every `[patch.<source>]` and `[replace]` entry in `manifest`, with the
/// package it overrides.
fn overrides(manifest: &toml::Table) -> Vec<(String, String)> {
    let mut found = Vec::new();
    if let Some(patches) = manifest.get("patch").and_then(toml::Value::as_table) {
        for (source, table) in patches {
            for (key, value) in table.as_table().into_iter().flatten() {
                found.push((
                    format!("[patch.{source:?}] {key}"),
                    dependency_package(key, value).to_owned(),
                ));
            }
        }
    }
    if let Some(replace) = manifest.get("replace").and_then(toml::Value::as_table) {
        for spec in replace.keys() {
            found.push((
                format!("[replace] {spec:?}"),
                spec_package_name(spec).to_owned(),
            ));
        }
    }
    found
}

/// The package a Cargo package ID spec names: `name`, `name@1.0`, `name:1.0`,
/// `<url>#name`, `<url>#name@1.0`, or a URL alone or with `#1.0`, which names
/// the URL's last path segment.
fn spec_package_name(spec: &str) -> &str {
    let Some(scheme_end) = spec.find("://") else {
        return spec.split(['@', ':']).next().unwrap_or(spec);
    };
    let (url, fragment) = spec.split_once('#').unwrap_or((spec, ""));
    let name = fragment.split(['@', ':']).next().unwrap_or("");
    if name.starts_with(|c: char| c.is_alphabetic()) {
        return name;
    }
    url[scheme_end + 3..]
        .split(['?', '#'])
        .next()
        .unwrap_or("")
        .trim_end_matches('/')
        .rsplit('/')
        .next()
        .unwrap_or("")
}

fn read_toml(path: &Path) -> Result<toml::Table, String> {
    let body = fs::read_to_string(path).map_err(display_err("read", path))?;
    toml::from_str(&body).map_err(|error| format!("parse {}: {error}", path.display()))
}

/// The directory of the workspace the route crate belongs to, found as Cargo
/// finds it: the route crate itself when it declares `[workspace]`, the
/// directory its `package.workspace` names, or the nearest ancestor that
/// declares `[workspace]`, which may be outside the Petal, as in a monorepo.
/// Like Cargo, the search passes over a workspace that excludes the crate
/// without also naming it in `members`, and follows an ancestor package's
/// `package.workspace`. With none, the route crate is its own workspace.
fn workspace_dir(crate_dir: &Path, manifest: &toml::Table) -> Result<PathBuf, String> {
    if manifest.contains_key("workspace") {
        return Ok(crate_dir.to_owned());
    }
    // `package.workspace` points at the workspace root from a package's own
    // directory.
    let pointer = |manifest: &toml::Table, dir: &Path| -> Option<Result<PathBuf, String>> {
        let path = manifest
            .get("package")?
            .get("workspace")?
            .as_str()
            .map(|path| dir.join(path))?;
        // Cargo applies `..` to the pointer as written.
        Some(Ok(lexical_normalize(&path)))
    };
    if let Some(root) = pointer(manifest, crate_dir) {
        return root;
    }
    for dir in crate_dir.ancestors().skip(1) {
        let path = dir.join("Cargo.toml");
        if !path.is_file() {
            continue;
        }
        let mut ancestor = read_toml(&path)?;
        let Some(workspace) = ancestor.remove("workspace") else {
            // Cargo also follows an ancestor package's pointer.
            if let Some(root) = pointer(&ancestor, dir) {
                return root;
            }
            continue;
        };
        // Cargo compares these entries as path prefixes, without globs.
        let names_the_crate = |key: &str| {
            workspace
                .get(key)
                .and_then(toml::Value::as_array)
                .into_iter()
                .flatten()
                .filter_map(toml::Value::as_str)
                .any(|entry| crate_dir.starts_with(dir.join(entry)))
        };
        if names_the_crate("exclude") && !names_the_crate("members") {
            continue;
        }
        return Ok(dir.to_owned());
    }
    Ok(crate_dir.to_owned())
}

/// Whether Cargo resolves `package` for the workspace of `manifest` to a local
/// path, as a `[patch]` in Cargo config does wherever Cargo finds it: in an
/// ancestor of the directory it runs in, through `include`, or in Cargo home.
/// This is how the petal repository's consumer CI builds a Petal against its
/// own checkout. `--locked` keeps Cargo from rewriting the lockfile, so with
/// no such patch it fails, and its error says why. `--color=never` keeps that
/// error readable when `CARGO_TERM_COLOR=always` is set, as it is in CI.
fn cargo_resolves_to_a_path(
    mut cargo: Command,
    manifest: &Path,
    package: &str,
) -> Result<bool, String> {
    let output = cargo
        .args(["metadata", "--format-version=1", "--locked"])
        .arg("--color=never")
        .arg("--manifest-path")
        .arg(manifest)
        .output()
        .map_err(|error| format!("run cargo metadata: {error}"))?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(stderr
            .lines()
            .find(|line| line.starts_with("error"))
            .unwrap_or("cargo metadata failed")
            .to_owned());
    }
    let metadata: serde_json::Value = serde_json::from_slice(&output.stdout)
        .map_err(|error| format!("parse cargo metadata: {error}"))?;
    Ok(metadata["packages"]
        .as_array()
        .into_iter()
        .flatten()
        .any(|candidate| candidate["name"] == package && candidate["source"].is_null()))
}

/// Check one `Cargo.lock` source against the pin.
fn lock_source_matches(expected: &SdkSource, source: &str, version: Option<&str>) -> bool {
    match expected {
        SdkSource::Path(_) => false,
        SdkSource::Version {
            requirement,
            registry: None,
        } => {
            CRATES_IO_SOURCES.contains(&source)
                && requirement
                    .strip_prefix('=')
                    .is_some_and(|exact| version == Some(exact))
        }
        SdkSource::Version { .. } => false,
        SdkSource::Git { url, rev, .. } => {
            let Some(locked) = source.strip_prefix("git+") else {
                return false;
            };
            let Some((locked, commit)) = locked.split_once('#') else {
                return false;
            };
            let (locked_url, query) = locked.split_once('?').unwrap_or((locked, ""));
            let locked_rev = query
                .split('&')
                .find_map(|pair| pair.strip_prefix("rev="))
                .map(percent_decode);
            normalize_git_url(locked_url) == *url
                && locked_rev.as_deref() == rev.as_deref()
                && rev
                    .as_deref()
                    .is_some_and(|expected| commit.eq_ignore_ascii_case(expected))
        }
    }
}

/// Keep generated components on the route crate's authoritative SDK source.
/// The optional `[sdk]` table is a migration assertion, not another input to
/// compilation. Check it and the following against the route dependency:
///
/// - every SDK dependency of the route crate, in any dependency table, and any
///   `workspace = true` entry resolved from its workspace;
/// - for a git pin, every dependency on another crate from the SDK's
///   repository (such as `bloom-petal-contract`), here and in the build
///   configuration's `[dependencies]`;
/// - every lockfile entry for those packages;
/// - that the route crate does not make the SDK optional;
/// - that the route crate and its workspace have no `[patch]` or `[replace]`
///   of any crate, because generated components ignore them.
///
/// A lockfile entry with no source is accepted only while Cargo, run from the
/// current directory as the component build is, resolves the SDK to a local
/// path, as a Cargo config `[patch]` does in the petal repository's consumer
/// CI. `PETAL_REQUIRE_LOCKED_SOURCES=1`, which the release workflow sets,
/// refuses such an entry; the component build then also checks every locked
/// package's real source (see `require_locked_sources`).
pub fn check_sdk_pin(root: impl AsRef<Path>, config: &BuildConfig) -> Result<(), String> {
    let allow_patch = config_overrides_allowed(std::env::var_os("PETAL_REQUIRE_LOCKED_SOURCES"))?;
    let cwd =
        std::env::current_dir().map_err(|error| format!("resolve current directory: {error}"))?;
    if !allow_patch {
        let found = checkout_cargo_config(&cwd, cargo_home().as_deref());
        if !found.is_empty() {
            return Err(format!(
                "PETAL_REQUIRE_LOCKED_SOURCES=1 refuses Cargo config, which can move packages, wrap or replace rustc, or change the linker, flags or environment without changing the lockfile: {}",
                found
                    .iter()
                    .map(|path| path.display().to_string())
                    .collect::<Vec<_>>()
                    .join(", ")
            ));
        }
        let toolchains = custom_toolchain_files(&cwd);
        if !toolchains.is_empty() {
            return Err(format!(
                "PETAL_REQUIRE_LOCKED_SOURCES=1 refuses a rust-toolchain file that names a toolchain by path, which can replace rustc: {}",
                toolchains
                    .iter()
                    .map(|path| path.display().to_string())
                    .collect::<Vec<_>>()
                    .join(", ")
            ));
        }
    }
    check_sdk_pin_with(root.as_ref(), config, allow_patch, |manifest, package| {
        let mut cargo = Command::new("cargo");
        cargo.current_dir(&cwd);
        cargo_resolves_to_a_path(cargo, manifest, package)
            .map_err(|error| format!("{error} (Cargo run from {})", cwd.display()))
    })
}

/// The rust-toolchain files rustup reads from `cwd` and its ancestors that
/// name a toolchain by path, which can replace rustc. A channel names a
/// toolchain rustup installs itself, which release builds accept.
fn custom_toolchain_files(cwd: &Path) -> Vec<PathBuf> {
    cwd.ancestors()
        .flat_map(|dir| ["rust-toolchain.toml", "rust-toolchain"].map(|name| dir.join(name)))
        .filter(|path| {
            let Ok(text) = fs::read_to_string(path) else {
                return false;
            };
            match toml::from_str::<toml::Table>(&text) {
                Ok(table) => table
                    .get("toolchain")
                    .and_then(|toolchain| toolchain.get("path"))
                    .is_some(),
                // The legacy form holds only a toolchain name.
                Err(_) => text.trim().contains(['/', '\\']),
            }
        })
        .collect()
}

/// The Cargo config files Cargo reads from `cwd` and its ancestors, apart
/// from Cargo home's own, which belongs to the machine rather than the
/// checkout.
fn checkout_cargo_config(cwd: &Path, cargo_home: Option<&Path>) -> Vec<PathBuf> {
    let resolved = |path: &Path| path.canonicalize().unwrap_or_else(|_| path.to_owned());
    let home = cargo_home.map(resolved);
    cwd.ancestors()
        .map(|dir| dir.join(".cargo"))
        .filter(|dir| home.as_ref().is_none_or(|home| resolved(dir) != *home))
        .flat_map(|dir| ["config", "config.toml"].map(|name| dir.join(name)))
        .filter(|path| path.exists())
        .collect()
}

/// Whether `PETAL_REQUIRE_LOCKED_SOURCES`, unset, `0` or `1`, permits Cargo
/// config to move locked packages, as a `[patch]` of the SDK to a local
/// checkout does. Any other value is refused rather than ignored.
fn config_overrides_allowed(value: Option<std::ffi::OsString>) -> Result<bool, String> {
    match value {
        None => Ok(true),
        Some(value) if value == "0" => Ok(true),
        Some(value) if value == "1" => Ok(false),
        Some(_) => Err("PETAL_REQUIRE_LOCKED_SOURCES must be 0 or 1".into()),
    }
}

/// [`check_sdk_pin`], asking `resolves_to_a_path` only about a lockfile entry
/// that puts the SDK at a local path the pin does not name, and only when
/// `allow_patch` permits one.
fn check_sdk_pin_with(
    root: &Path,
    config: &BuildConfig,
    allow_patch: bool,
    resolves_to_a_path: impl Fn(&Path, &str) -> Result<bool, String>,
) -> Result<(), String> {
    let package = &config.sdk.package;
    let resolved = route_sdk_dependency(root, config)?;
    let expected = dependency_source(root, package, &resolved)?;
    match &expected {
        SdkSource::Git {
            rev: Some(rev),
            other_ref: None,
            ..
        } if is_full_commit(rev) => {}
        SdkSource::Version {
            requirement,
            registry: None,
        } if is_exact_version(requirement) => {}
        SdkSource::Path(_) => {}
        _ => {
            return Err(format!(
                "route SDK requires an immutable source, got {expected}"
            ));
        }
    }
    if (config.sdk.version.is_some() || config.sdk.path.is_some() || config.sdk.git.is_some())
        && configured_sdk_source(root, &config.sdk)? != expected
    {
        return Err(format!(
            "SDK pinned inconsistently: [sdk] {}; route/Cargo.toml SDK {expected}. Update the route dependency and remove or align [sdk].",
            configured_sdk_source(root, &config.sdk)?
        ));
    }
    let sdk_repo = match &expected {
        SdkSource::Git { url, .. } => Some(url.clone()),
        _ => None,
    };
    let same_repo = |source: &SdkSource| match (source, &sdk_repo) {
        (SdkSource::Git { url, .. }, Some(repo)) => url == repo,
        _ => false,
    };
    let shown = |path: &Path| {
        path.strip_prefix(root)
            .unwrap_or(path)
            .display()
            .to_string()
    };

    let crate_dir = root.join(&config.route_crate.path);
    let manifest_path = crate_dir.join("Cargo.toml");
    let manifest = read_toml(&manifest_path)?;
    let workspace = workspace_dir(&crate_dir, &manifest)?;
    let workspace_manifest_path = workspace.join("Cargo.toml");
    let workspace_manifest = if workspace == crate_dir {
        None
    } else {
        Some(read_toml(&workspace_manifest_path)?)
    };
    let workspace_dependencies = workspace_manifest
        .as_ref()
        .unwrap_or(&manifest)
        .get("workspace")
        .and_then(|workspace| workspace.get("dependencies"))
        .and_then(toml::Value::as_table);

    let mut disagreements = Vec::new();
    // Packages from the SDK's source, whose lockfile entries are checked too.
    let mut checked_packages = BTreeSet::from([package.clone()]);
    let mut check_entry = |label: String,
                           base: &Path,
                           key: &str,
                           value: &toml::Value,
                           disagreements: &mut Vec<String>|
     -> Result<(), String> {
        let named = dependency_package(key, value);
        let source = dependency_source(base, key, value)?;
        if named == package.as_str() {
            if source != expected {
                disagreements.push(format!("{label}: {source}"));
            }
        } else if same_repo(&source) {
            checked_packages.insert(named.to_owned());
            if let (SdkSource::Git { rev, other_ref, .. }, SdkSource::Git { rev: want, .. }) =
                (&source, &expected)
            {
                if rev != want || other_ref.is_some() {
                    disagreements.push(format!("{label}: {source} (same repository as the SDK)"));
                }
            }
        }
        Ok(())
    };

    for (section, table) in dependency_tables(&manifest) {
        for (key, value) in table {
            let label = format!("{} {section} {key}", shown(&manifest_path));
            if value.get("workspace").and_then(toml::Value::as_bool) == Some(true) {
                let Some(inherited) = workspace_dependencies.and_then(|deps| deps.get(key)) else {
                    return Err(format!(
                        "{label} inherits from a workspace with no [workspace.dependencies] {key}"
                    ));
                };
                check_entry(
                    format!(
                        "{label} (from {} [workspace.dependencies])",
                        shown(&workspace_manifest_path)
                    ),
                    &workspace,
                    key,
                    inherited,
                    &mut disagreements,
                )?;
                continue;
            }
            check_entry(label, &crate_dir, key, value, &mut disagreements)?;
        }
    }
    for (key, value) in &config.dependencies {
        check_entry(
            format!("[dependencies] {key} in the build configuration"),
            root,
            key,
            value,
            &mut disagreements,
        )?;
    }
    // Generated route components ignore every override in the route crate and
    // its workspace, whichever crate it moves, so the route's tests would run
    // a dependency graph the components do not.
    let mut overridden = Vec::new();
    for (path, table) in [
        (&manifest_path, Some(&manifest)),
        (&workspace_manifest_path, workspace_manifest.as_ref()),
    ] {
        let Some(table) = table else {
            continue;
        };
        for (entry, name) in overrides(table) {
            overridden.push(format!(
                "{} {entry} overrides {name}, which generated route components ignore",
                shown(path)
            ));
        }
    }

    let lock_path = workspace.join("Cargo.lock");
    if lock_path.is_file() {
        let lock = read_toml(&lock_path)?;
        let entries = lock
            .get("package")
            .and_then(toml::Value::as_array)
            .map(Vec::as_slice)
            .unwrap_or_default();
        // Whether Cargo resolves each package to a local path, asked once.
        let mut patched = BTreeMap::new();
        for entry in entries.iter().filter_map(toml::Value::as_table) {
            let Some(name) = entry.get("name").and_then(toml::Value::as_str) else {
                continue;
            };
            let version = entry.get("version").and_then(toml::Value::as_str);
            let source = entry.get("source").and_then(toml::Value::as_str);
            // Cargo records every `[replace]` here, however its key is spelled.
            // Generated components ignore them, so the route's tests would run
            // code the components do not, whichever crate is replaced.
            if let Some(replacement) = entry.get("replace").and_then(toml::Value::as_str) {
                overridden.push(format!(
                    "{} replaces {name} {} with {replacement}, which generated route components ignore",
                    shown(&lock_path),
                    version.unwrap_or("?"),
                ));
                continue;
            }
            let mut reason = String::new();
            // The SDK and the other crates from its repository are held to the
            // same source: a pinned path, an active patch, or the pinned rev.
            let matches = if checked_packages.contains(name) {
                match source {
                    // A local path: only a pinned path or an active patch.
                    None if matches!(expected, SdkSource::Path(_)) => true,
                    None if !allow_patch => {
                        reason = ", which PETAL_REQUIRE_LOCKED_SOURCES=1 refuses".into();
                        false
                    }
                    None => match patched
                        .entry(name)
                        .or_insert_with(|| resolves_to_a_path(&manifest_path, name))
                    {
                        Ok(true) => true,
                        Ok(false) => {
                            reason = ", which Cargo does not resolve it to".into();
                            false
                        }
                        // `--locked` fails whenever the lockfile is out of date,
                        // not only when no patch applies.
                        Err(error) => {
                            reason = format!(
                                ", and Cargo could not confirm that a config [patch] puts it there: {error}"
                            );
                            false
                        }
                    },
                    Some(source) if name == package.as_str() => {
                        lock_source_matches(&expected, source, version)
                    }
                    // Another crate's version is its own; its source and rev
                    // must still be the SDK's.
                    Some(source) => {
                        source.starts_with("git+") && lock_source_matches(&expected, source, None)
                    }
                }
            } else {
                true
            };
            if !matches {
                disagreements.push(format!(
                    "{} resolves {name} {} from {}{reason}",
                    shown(&lock_path),
                    version.unwrap_or("?"),
                    source.unwrap_or("a local path")
                ));
            }
        }
    }

    let list = |lines: &[String]| {
        lines
            .iter()
            .map(|line| format!("  - {line}"))
            .collect::<Vec<_>>()
            .join("\n")
    };
    let mut errors = Vec::new();
    if !disagreements.is_empty() {
        errors.push(format!(
            "{package} is pinned inconsistently with the route SDK {expected}:\n{}\nAlign these sources, then refresh the lockfile:\n  cargo update --manifest-path {} -p {package}",
            list(&disagreements),
            shown(&workspace_manifest_path),
        ));
    }
    if !overridden.is_empty() {
        errors.push(format!(
            "the route workspace overrides dependencies that generated route components build without:\n{}\nRemove these overrides, then run the route's tests again to refresh the lockfile.",
            list(&overridden),
        ));
    }
    if errors.is_empty() {
        Ok(())
    } else {
        Err(errors.join("\n"))
    }
}

fn path_from_member(member: &Path, target: &Path) -> Result<String, String> {
    let from = member.components().collect::<Vec<_>>();
    let to = target.components().collect::<Vec<_>>();
    let common = from
        .iter()
        .zip(&to)
        .take_while(|(left, right)| left == right)
        .count();
    if common == 0 {
        return Ok(target.to_string_lossy().into_owned());
    }
    let mut relative = PathBuf::new();
    for _ in common..from.len() {
        relative.push("..");
    }
    for component in &to[common..] {
        relative.push(component.as_os_str());
    }
    relative
        .to_str()
        .map(str::to_owned)
        .ok_or_else(|| format!("path is not UTF-8: {}", target.display()))
}

/// Install `staging` at `destination`, moving any previous output aside to a
/// sibling of `staging`, which is returned.
fn replace_dir(staging: &Path, destination: &Path) -> Result<Option<PathBuf>, String> {
    // The fresh generation owns this sibling name under target/, outside the
    // package tree. Preserve previous output rather than recursively deleting
    // it: unrelated files cannot disappear or enter the new runtime tree.
    let backup = staging.with_extension("previous");
    if backup.exists() {
        return Err(format!(
            "refusing to overwrite retained output {}",
            backup.display()
        ));
    }
    let had_destination = destination.exists();
    if had_destination {
        fs::rename(destination, &backup)
            .map_err(|error| format!("backup {}: {error}", destination.display()))?;
    }
    if let Err(error) = fs::rename(staging, destination) {
        if had_destination {
            let _ = fs::rename(&backup, destination);
        }
        return Err(format!("install {}: {error}", destination.display()));
    }
    Ok(had_destination.then_some(backup))
}

/// The regular files under `root`, by path relative to it, when `root` is a
/// real directory holding only real directories and regular files. `None`
/// when the root or anything in it is a link or another kind of file, since
/// a link can reach outside the tree.
fn owned_tree(root: &Path) -> Result<Option<BTreeMap<String, PathBuf>>, String> {
    if !fs::symlink_metadata(root)
        .map_err(display_err("inspect", root))?
        .is_dir()
    {
        return Ok(None);
    }
    let mut files = BTreeMap::new();
    let mut pending = vec![root.to_owned()];
    while let Some(directory) = pending.pop() {
        for entry in fs::read_dir(&directory).map_err(display_err("read", &directory))? {
            let path = entry.map_err(|error| error.to_string())?.path();
            let metadata = fs::symlink_metadata(&path).map_err(display_err("inspect", &path))?;
            if metadata.is_dir() {
                pending.push(path);
            } else if metadata.is_file() {
                let relative = path
                    .strip_prefix(root)
                    .map_err(|error| error.to_string())?
                    .to_string_lossy()
                    .replace('\\', "/");
                files.insert(relative, path);
            } else {
                return Ok(None);
            }
        }
    }
    Ok(Some(files))
}

/// The folder metadata macOS Finder writes into any directory it shows.
const FINDER_METADATA: &str = ".DS_Store";

/// Remove `backup` when it holds exactly the components that `report_path`
/// records an earlier build writing to this output, file for file and byte
/// for byte, whichever Petal that build was for: it holds nothing but builder
/// output. Finder's `.DS_Store` does not count and goes with it. Anything
/// else, such as a file the builder did not write, a component changed since,
/// a link or another output's report, keeps the whole backup for its owner.
fn discard_rebuilt_backup(
    backup: &Path,
    report_path: &Path,
    config: &BuildConfig,
) -> Result<bool, String> {
    let Ok(body) = fs::read_to_string(report_path) else {
        return Ok(false);
    };
    let Ok(report) = serde_json::from_str::<serde_json::Value>(&body) else {
        return Ok(false);
    };
    let (Some(output), Some(artifacts)) =
        (report["output"].as_str(), report["artifacts"].as_object())
    else {
        return Ok(false);
    };
    if Path::new(output) != config.output {
        return Ok(false);
    }
    let Some(files) = owned_tree(backup)? else {
        return Ok(false);
    };
    let components = files
        .iter()
        .filter(|(relative, _)| {
            Path::new(relative).file_name() != Some(OsStr::new(FINDER_METADATA))
        })
        .collect::<Vec<_>>();
    if components.len() != artifacts.len() {
        return Ok(false);
    }
    for (relative, path) in components {
        let Some(recorded) = artifacts.get(relative).and_then(serde_json::Value::as_str) else {
            return Ok(false);
        };
        let bytes = fs::read(path).map_err(display_err("read", path))?;
        if blake3::hash(&bytes).to_hex().as_str() != recorded {
            return Ok(false);
        }
    }
    // std's remove_dir_all never follows a link, even one swapped in after
    // the check above, so it cannot reach outside the copy. Anything added to
    // the copy since goes too; it is still inside target/petal-routes/staging/.
    fs::remove_dir_all(backup).map_err(display_err("remove", backup))?;
    Ok(true)
}

/// Where a build records the components it wrote to its output. Each output
/// has one report, whichever Petal built it last, so a build recognises the
/// output it replaces whichever configurations built in the same root in
/// between.
fn output_report_path(build_root: &Path, config: &BuildConfig) -> PathBuf {
    let output = config
        .output
        .components()
        .map(|component| component.as_os_str().to_string_lossy())
        .collect::<Vec<_>>()
        .join("/");
    let key = blake3::hash(output.as_bytes()).to_hex();
    build_root
        .join("reports")
        .join(format!("{}.json", &key[..16]))
}

/// Discard `backup` when any of `reports` shows it holds nothing but builder
/// output, and say so when it is kept. Each is tried in turn: an older
/// builder updates only the legacy report, so this output's own report can
/// describe an earlier build than the one replaced.
fn retire_backup(backup: &Path, reports: &[&Path], config: &BuildConfig) -> bool {
    for report in reports {
        match discard_rebuilt_backup(backup, report, config) {
            Ok(true) => return true,
            Ok(false) => {}
            Err(error) => {
                eprintln!("previous output retained at {}: {error}", backup.display());
                return false;
            }
        }
    }
    eprintln!("previous output retained at {}", backup.display());
    false
}

/// Create the fresh generation `staging`, write each route's component into
/// it with `write`, check the set, and install it at `out_dir`, returning the
/// backup of what it replaced. A generation that fails is removed again, so
/// failed builds leave nothing under `target/petal-routes/staging/`.
fn install_generation(
    staging: &Path,
    out_dir: &Path,
    routes: &[Route],
    write: impl Fn(&Route, &Path) -> Result<(), String>,
) -> Result<Option<PathBuf>, String> {
    let parent = staging.parent().ok_or("staging directory has no parent")?;
    fs::create_dir_all(parent).map_err(display_err("create", parent))?;
    fs::create_dir(staging).map_err(display_err("create", staging))?;
    let result = (|| {
        let failures = routes
            .iter()
            .filter_map(|route| {
                let output = staging.join(format!("{}.wasm", route.path));
                let parent = output.parent().unwrap_or(staging);
                fs::create_dir_all(parent)
                    .map_err(display_err("create", parent))
                    .and_then(|()| write(route, &output))
                    .err()
            })
            .collect::<Vec<_>>();
        if !failures.is_empty() {
            return Err(failures.join("\n"));
        }
        let staged = discover_wasm_paths(staging)?;
        let expected = routes
            .iter()
            .map(|route| format!("{}.wasm", route.path))
            .collect::<BTreeSet<_>>();
        if staged != expected {
            return Err("staging route set differs from discovered controllers".into());
        }
        let parent = out_dir.parent().ok_or("output directory has no parent")?;
        fs::create_dir_all(parent).map_err(display_err("create", parent))?;
        replace_dir(staging, out_dir)
    })();
    if result.is_err() {
        discard_failed_staging(staging);
    }
    result
}

/// Remove a generation that did not become the output. This build created it
/// and wrote everything in it, and std's remove_dir_all never follows a link.
fn discard_failed_staging(staging: &Path) {
    match fs::remove_dir_all(staging) {
        Ok(()) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => eprintln!(
            "failed build output retained at {}: {error}",
            staging.display()
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn package_config() -> BuildConfig {
        BuildConfig {
            schema: "bloom.petal.build.v1".into(),
            name: "demo".into(),
            routes: "route/files".into(),
            output: "petal/demo".into(),
            route_crate: RouteCrate {
                package: "demo-route".into(),
                path: "route".into(),
                alias: "demo_route".into(),
            },
            sdk: SdkDependency {
                package: "bloom-petal-sdk".into(),
                version: Some("=0.1.0".into()),
                path: None,
                git: None,
                rev: None,
            },
            dependencies: BTreeMap::new(),
            jobs: None,
        }
    }

    const SDK_GIT: &str = "https://github.com/bloom-directory/petal";
    const REV_A: &str = "222557410bf4d45b0fc30feefe779d4234c04097";
    const REV_B: &str = "2beed2f0000000000000000000000000000000aa";

    fn git_sdk(rev: &str) -> SdkDependency {
        SdkDependency {
            package: "bloom-petal-sdk".into(),
            version: None,
            path: None,
            git: Some(SDK_GIT.into()),
            rev: Some(rev.into()),
        }
    }

    /// A package root holding only what the pin check reads: the route crate's
    /// manifest and, when given, its lockfile.
    fn pin_fixture(
        sdk: SdkDependency,
        manifest: &str,
        lock: Option<&str>,
    ) -> (tempfile::TempDir, BuildConfig) {
        let tmp = tempfile::tempdir().unwrap();
        let route = tmp.path().join("route");
        fs::create_dir_all(&route).unwrap();
        fs::write(
            route.join("Cargo.toml"),
            format!("[package]\nname = \"demo-route\"\nversion = \"0.1.0\"\nedition = \"2024\"\n\n{manifest}"),
        )
        .unwrap();
        if let Some(lock) = lock {
            fs::write(route.join("Cargo.lock"), lock).unwrap();
        }
        let mut config = package_config();
        config.sdk = sdk;
        (tmp, config)
    }

    fn git_lock(rev: &str) -> String {
        format!(
            "version = 4\n\n[[package]]\nname = \"bloom-petal-sdk\"\nversion = \"0.1.0\"\nsource = \"git+{SDK_GIT}?rev={rev}#{rev}\"\n\n[[package]]\nname = \"demo-route\"\nversion = \"0.1.0\"\n"
        )
    }

    #[test]
    fn sdk_source_without_build_pin_preserves_workspace_features() {
        let (tmp, mut config) = pin_fixture(
            SdkDependency::default(),
            "[dependencies]\nrenamed = { workspace = true, features = [\"consumer\"] }\n",
            None,
        );
        fs::write(tmp.path().join("Cargo.toml"), format!(
            "[workspace]\nmembers = [\"route\"]\n[workspace.dependencies]\nrenamed = {{ package = \"bloom-petal-sdk\", git = \"{SDK_GIT}\", rev = \"{REV_A}\", default-features = false, features = [\"shared\"] }}\n"
        )).unwrap();
        check_sdk_pin(tmp.path(), &config).unwrap();
        let dependency = route_sdk_dependency(tmp.path(), &config).unwrap();
        assert_eq!(dependency["rev"].as_str(), Some(REV_A));
        assert_eq!(dependency["default-features"].as_bool(), Some(false));
        assert_eq!(
            dependency["features"].as_array().unwrap(),
            &vec![
                toml::Value::String("shared".into()),
                toml::Value::String("consumer".into())
            ]
        );
        config.sdk = git_sdk(REV_B);
        assert!(
            check_sdk_pin(tmp.path(), &config)
                .unwrap_err()
                .contains("pinned inconsistently")
        );
    }

    #[test]
    fn component_graph_may_add_wrappers_but_not_re_resolve_dependencies() {
        let original: toml::Table = toml::from_str("version = 4\n[[package]]\nname = \"dependency\"\nversion = \"1.0.0\"\nsource = \"registry+https://github.com/rust-lang/crates.io-index\"\nchecksum = \"abc\"\n").unwrap();
        let mut generated = original.clone();
        generated["package"]
            .as_array_mut()
            .unwrap()
            .push(toml::Value::Table(toml::Table::from_iter([
                ("name".into(), toml::Value::String("wrapper".into())),
                ("version".into(), toml::Value::String("0.1.0".into())),
            ])));
        let wrappers = BTreeSet::from(["wrapper"]);
        validate_locked_graph(&original, &generated, &wrappers).unwrap();
        generated["package"].as_array_mut().unwrap()[0]["version"] =
            toml::Value::String("1.1.0".into());
        assert!(
            validate_locked_graph(&original, &generated, &wrappers)
                .unwrap_err()
                .contains("dependency differs")
        );
        generated = original.clone();
        generated["package"].as_array_mut().unwrap()[0]["checksum"] =
            toml::Value::String("tampered".into());
        assert!(validate_locked_graph(&original, &generated, &wrappers).is_err());
        // Another source with the same version and checksum is another package.
        generated = original.clone();
        generated["package"].as_array_mut().unwrap()[0]["source"] =
            toml::Value::String("registry+https://example.invalid/index".into());
        assert!(
            validate_locked_graph(&original, &generated, &wrappers)
                .unwrap_err()
                .contains("dependency differs")
        );
    }

    #[test]
    fn component_graph_prunes_dev_dependencies_without_switching_existing_versions() {
        let original: toml::Table = toml::from_str("version = 4\n[[package]]\nname = \"route\"\nversion = \"0.1.0\"\ndependencies = [\"dependency 1.0.0\", \"dev\"]\n[[package]]\nname = \"dependency\"\nversion = \"1.0.0\"\n[[package]]\nname = \"dependency\"\nversion = \"2.0.0\"\n[[package]]\nname = \"dev\"\nversion = \"1.0.0\"\n").unwrap();
        let mut generated = original.clone();
        generated["package"].as_array_mut().unwrap()[0]["dependencies"] =
            toml::Value::Array(vec![toml::Value::String("dependency 1.0.0".into())]);
        validate_locked_graph(&original, &generated, &BTreeSet::new()).unwrap();
        generated["package"].as_array_mut().unwrap()[0]["dependencies"] =
            toml::Value::Array(vec![toml::Value::String("dependency 2.0.0".into())]);
        assert!(
            validate_locked_graph(&original, &generated, &BTreeSet::new())
                .unwrap_err()
                .contains("dependency edges for route")
        );
    }

    #[test]
    fn replacing_components_removes_stale_output_and_preserves_unrelated_files() {
        let tmp = tempfile::tempdir().unwrap();
        let staging = tmp.path().join("target/generation");
        let output = tmp.path().join("petal/demo");
        fs::create_dir_all(&staging).unwrap();
        fs::create_dir_all(&output).unwrap();
        fs::write(staging.join("new.wasm"), b"new component").unwrap();
        fs::write(output.join("old.wasm"), b"stale component").unwrap();
        fs::write(output.join("notes.txt"), b"preserve me").unwrap();
        assert_eq!(
            replace_dir(&staging, &output).unwrap(),
            Some(staging.with_extension("previous"))
        );
        assert!(output.join("new.wasm").is_file());
        assert!(!output.join("old.wasm").exists());
        assert_eq!(
            fs::read(staging.with_extension("previous").join("notes.txt")).unwrap(),
            b"preserve me"
        );
        let missing = tmp.path().join("target/missing");
        assert!(replace_dir(&missing, &output).is_err());
        assert_eq!(fs::read(output.join("new.wasm")).unwrap(), b"new component");

        // A first build replaces nothing, so there is no backup to retire.
        let first = tmp.path().join("target/first");
        fs::create_dir_all(&first).unwrap();
        let fresh = tmp.path().join("petal/fresh");
        assert_eq!(replace_dir(&first, &fresh).unwrap(), None);
        assert!(fresh.is_dir());
        assert!(!first.with_extension("previous").exists());
    }

    /// A build report for `petal`'s `output` recording `components`.
    fn report_body(petal: &str, output: &str, components: &[(&str, &[u8])]) -> String {
        let artifacts = components
            .iter()
            .map(|(name, bytes)| ((*name).to_owned(), blake3::hash(bytes).to_hex().to_string()))
            .collect::<BTreeMap<_, _>>();
        serde_json::json!({ "petal": petal, "output": output, "artifacts": artifacts }).to_string()
    }

    /// A backup of `status.wasm` and `nested/plan.md.wasm`, and a build report
    /// for `petal`'s `petal/demo` recording `components`.
    fn backup_fixture(
        root: &Path,
        petal: &str,
        components: &[(&str, &[u8])],
    ) -> (PathBuf, PathBuf) {
        let backup = root.join("staging/demo-1-2.previous");
        fs::create_dir_all(backup.join("nested")).unwrap();
        fs::write(backup.join("status.wasm"), b"status").unwrap();
        fs::write(backup.join("nested/plan.md.wasm"), b"plan").unwrap();
        let report = root.join("build-report.json");
        fs::write(&report, report_body(petal, "petal/demo", components)).unwrap();
        (backup, report)
    }

    const REPORTED: &[(&str, &[u8])] =
        &[("status.wasm", b"status"), ("nested/plan.md.wasm", b"plan")];

    /// Whichever Petal's build recorded them, the components are all builder
    /// output.
    #[test]
    fn a_backup_of_exactly_the_reported_components_is_discarded() {
        for petal in ["demo", "other"] {
            let tmp = tempfile::tempdir().unwrap();
            let (backup, report) = backup_fixture(tmp.path(), petal, REPORTED);
            assert!(
                discard_rebuilt_backup(&backup, &report, &package_config()).unwrap(),
                "{petal}"
            );
            assert!(!backup.exists(), "{petal}");
            assert!(tmp.path().join("staging").is_dir(), "{petal}");
        }
    }

    #[test]
    fn a_backup_holding_anything_a_rebuild_cannot_restore_is_kept_whole() {
        for case in [
            "extra file",
            "changed component",
            "missing component",
            "no report",
            "another output",
        ] {
            let tmp = tempfile::tempdir().unwrap();
            let (backup, report) = backup_fixture(tmp.path(), "demo", REPORTED);
            match case {
                "extra file" => fs::write(backup.join("notes.txt"), b"keep me").unwrap(),
                "changed component" => fs::write(backup.join("status.wasm"), b"edited").unwrap(),
                "missing component" => fs::remove_file(backup.join("nested/plan.md.wasm")).unwrap(),
                "no report" => fs::remove_file(&report).unwrap(),
                "another output" => {
                    fs::write(&report, report_body("demo", "petal/other", REPORTED)).unwrap()
                }
                _ => {}
            }
            assert!(
                !discard_rebuilt_backup(&backup, &report, &package_config()).unwrap(),
                "{case}"
            );
            assert!(backup.join("status.wasm").is_file(), "{case}");
            if case != "missing component" {
                assert!(backup.join("nested/plan.md.wasm").is_file(), "{case}");
            }
        }
    }

    #[test]
    fn finder_metadata_does_not_keep_a_backup() {
        let tmp = tempfile::tempdir().unwrap();
        let (backup, report) = backup_fixture(tmp.path(), "demo", REPORTED);
        fs::write(backup.join(".DS_Store"), b"finder").unwrap();
        fs::write(backup.join("nested/.DS_Store"), b"finder").unwrap();
        assert!(discard_rebuilt_backup(&backup, &report, &package_config()).unwrap());
        assert!(!backup.exists());
    }

    /// Configurations building in one root keep one report per output, so a
    /// build retires its backup whichever ones built in between, including
    /// another Petal's build of the same output.
    #[test]
    fn each_output_keeps_its_own_report() {
        let tmp = tempfile::tempdir().unwrap();
        let demo = package_config();
        let mut other = package_config();
        other.output = "fixtures/components".into();
        let mut slashed = package_config();
        slashed.output = "petal/demo/".into();
        let mut renamed = package_config();
        renamed.name = "renamed".into();
        let own = output_report_path(tmp.path(), &demo);
        assert_ne!(own, output_report_path(tmp.path(), &other));
        assert_eq!(own, output_report_path(tmp.path(), &slashed));
        assert_eq!(own, output_report_path(tmp.path(), &renamed));

        // The other configuration built last, so build-report.json is its.
        let (backup, legacy) = backup_fixture(tmp.path(), "demo", REPORTED);
        fs::write(
            &legacy,
            report_body("demo", "fixtures/components", REPORTED),
        )
        .unwrap();
        assert!(!retire_backup(&backup, &[&own, &legacy], &demo));
        fs::create_dir_all(own.parent().unwrap()).unwrap();
        fs::write(&own, report_body("renamed", "petal/demo", REPORTED)).unwrap();
        assert!(retire_backup(&backup, &[&own, &legacy], &demo));
        assert!(!backup.exists());
    }

    /// An older builder updates only build-report.json, so this output's own
    /// report can describe an earlier build than the one replaced.
    #[test]
    fn a_stale_output_report_does_not_hide_a_matching_legacy_report() {
        let tmp = tempfile::tempdir().unwrap();
        let demo = package_config();
        let own = output_report_path(tmp.path(), &demo);
        let (backup, legacy) = backup_fixture(tmp.path(), "demo", REPORTED);
        fs::create_dir_all(own.parent().unwrap()).unwrap();
        let earlier: &[(&str, &[u8])] = &[
            ("status.wasm", b"earlier"),
            ("nested/plan.md.wasm", b"plan"),
        ];
        fs::write(&own, report_body("demo", "petal/demo", earlier)).unwrap();
        assert!(retire_backup(&backup, &[&own, &legacy], &demo));
        assert!(!backup.exists());
    }

    #[test]
    fn a_failed_generation_leaves_nothing_in_staging_and_the_output_untouched() {
        let tmp = tempfile::tempdir().unwrap();
        let staging = tmp.path().join("target/petal-routes/staging/demo-1-2");
        let output = tmp.path().join("petal/demo");
        fs::create_dir_all(&output).unwrap();
        fs::write(output.join("status.json.wasm"), b"installed").unwrap();
        let routes = [route("status.json"), route("drafts/[id]/plan.md")];
        let error = install_generation(&staging, &output, &routes, |route, path| {
            if route.path.ends_with("plan.md") {
                return Err("wasm-tools validate failed".into());
            }
            fs::write(path, b"new").map_err(|error| error.to_string())
        })
        .unwrap_err();
        assert!(error.contains("wasm-tools validate failed"), "{error}");
        assert!(!staging.exists());
        assert!(staging.parent().unwrap().is_dir());
        assert_eq!(
            fs::read(output.join("status.json.wasm")).unwrap(),
            b"installed"
        );

        let backup = install_generation(&staging, &output, &routes, |_, path| {
            fs::write(path, b"new").map_err(|error| error.to_string())
        })
        .unwrap();
        assert_eq!(backup, Some(staging.with_extension("previous")));
        assert_eq!(
            fs::read(output.join("drafts/[id]/plan.md.wasm")).unwrap(),
            b"new"
        );
    }

    #[cfg(unix)]
    #[test]
    fn a_backup_holding_a_link_is_kept_whole() {
        // A link in place of a component, or beside exactly the components.
        for beside in [false, true] {
            let tmp = tempfile::tempdir().unwrap();
            let (backup, report) = backup_fixture(tmp.path(), "demo", REPORTED);
            let outside = tmp.path().join("outside.wasm");
            fs::write(&outside, b"plan").unwrap();
            let link = if beside {
                backup.join("nested/link.wasm")
            } else {
                fs::remove_file(backup.join("nested/plan.md.wasm")).unwrap();
                backup.join("nested/plan.md.wasm")
            };
            std::os::unix::fs::symlink(&outside, &link).unwrap();
            assert!(
                !discard_rebuilt_backup(&backup, &report, &package_config()).unwrap(),
                "beside: {beside}"
            );
            assert!(backup.join("status.wasm").is_file(), "beside: {beside}");
            assert!(fs::symlink_metadata(&link).is_ok(), "beside: {beside}");
            assert!(outside.is_file(), "beside: {beside}");
        }
    }

    /// `replace_dir` moves a linked output as the link itself, so the backup
    /// root can be a link to a directory outside `target/`.
    #[cfg(unix)]
    #[test]
    fn a_linked_backup_root_is_kept_and_nothing_is_removed_through_it() {
        let tmp = tempfile::tempdir().unwrap();
        let (backup, report) = backup_fixture(tmp.path(), "demo", REPORTED);
        let elsewhere = tmp.path().join("elsewhere");
        fs::rename(&backup, &elsewhere).unwrap();
        std::os::unix::fs::symlink(&elsewhere, &backup).unwrap();
        assert!(!discard_rebuilt_backup(&backup, &report, &package_config()).unwrap());
        assert!(elsewhere.join("status.wasm").is_file());
        assert!(elsewhere.join("nested/plan.md.wasm").is_file());
    }

    #[test]
    fn sdk_pin_refuses_a_lockfile_that_names_the_pin_but_resolves_another_commit() {
        let manifest = format!(
            "[dependencies]\npetal = {{ package = \"bloom-petal-sdk\", git = \"{SDK_GIT}\", rev = \"{REV_A}\" }}\n"
        );
        let lock = git_lock(REV_A).replace(&format!("#{REV_A}"), &format!("#{REV_B}"));
        let (tmp, config) = pin_fixture(git_sdk(REV_A), &manifest, Some(&lock));
        assert!(
            check_sdk_pin(tmp.path(), &config)
                .unwrap_err()
                .contains("Cargo.lock resolves bloom-petal-sdk")
        );
    }

    #[test]
    fn locked_git_edges_resolve_to_the_full_commit_without_switching_sources() {
        let source = format!("git+{SDK_GIT}?rev={REV_A}");
        let registry = "registry+https://github.com/rust-lang/crates.io-index";
        let original: toml::Table = toml::from_str(&format!(
            "version = 4\n[[package]]\nname = \"route\"\nversion = \"0.1.0\"\ndependencies = [\"dependency 1.0.0 ({source})\"]\n[[package]]\nname = \"dependency\"\nversion = \"1.0.0\"\nsource = \"{source}#{REV_A}\"\n[[package]]\nname = \"dependency\"\nversion = \"1.0.0\"\nsource = \"{registry}\"\n"
        )).unwrap();
        validate_locked_graph(&original, &original, &BTreeSet::new()).unwrap();
        let mut changed = original.clone();
        changed["package"].as_array_mut().unwrap()[0]["dependencies"] =
            toml::Value::Array(vec![toml::Value::String(format!(
                "dependency 1.0.0 ({registry})"
            ))]);
        assert!(
            validate_locked_graph(&original, &changed, &BTreeSet::new())
                .unwrap_err()
                .contains("dependency edges for route")
        );
    }

    #[test]
    fn sdk_pin_agrees_across_build_config_manifest_and_lockfile() {
        let manifest = format!(
            "[dependencies]\npetal = {{ package = \"bloom-petal-sdk\", git = \"{SDK_GIT}\", rev = \"{REV_A}\" }}\n"
        );
        let (tmp, config) = pin_fixture(git_sdk(REV_A), &manifest, Some(&git_lock(REV_A)));
        check_sdk_pin(tmp.path(), &config).unwrap();
    }

    #[test]
    fn sdk_pin_refuses_a_route_crate_on_another_rev() {
        let manifest = format!(
            "[dependencies]\npetal = {{ package = \"bloom-petal-sdk\", git = \"{SDK_GIT}\", rev = \"{REV_B}\" }}\n"
        );
        let (tmp, config) = pin_fixture(git_sdk(REV_A), &manifest, None);
        let error = check_sdk_pin(tmp.path(), &config).unwrap_err();
        assert!(error.contains(&format!("rev {REV_A}")), "{error}");
        assert!(error.contains("route/Cargo.toml SDK"), "{error}");
        assert!(error.contains(&format!("rev {REV_B}")), "{error}");
    }

    #[test]
    fn sdk_pin_refuses_a_stale_lockfile() {
        let manifest = format!(
            "[dependencies]\npetal = {{ package = \"bloom-petal-sdk\", git = \"{SDK_GIT}\", rev = \"{REV_A}\" }}\n"
        );
        let (tmp, config) = pin_fixture(git_sdk(REV_A), &manifest, Some(&git_lock(REV_B)));
        let error = check_sdk_pin(tmp.path(), &config).unwrap_err();
        assert!(
            error.contains("route/Cargo.lock resolves bloom-petal-sdk"),
            "{error}"
        );
        assert!(error.contains(REV_B), "{error}");
        assert!(
            error.contains("cargo update --manifest-path route/Cargo.toml -p bloom-petal-sdk"),
            "{error}"
        );
    }

    fn git_dep(name: &str, package: &str, rev: &str) -> String {
        format!("{name} = {{ package = \"{package}\", git = \"{SDK_GIT}\", rev = \"{rev}\" }}\n")
    }

    #[test]
    fn sdk_pin_refuses_branches_and_tags() {
        for reference in ["branch = \"master\"", "tag = \"v0.1.0\""] {
            let manifest = format!(
                "[dependencies]\npetal = {{ package = \"bloom-petal-sdk\", git = \"{SDK_GIT}\", {reference} }}\n"
            );
            let (tmp, config) = pin_fixture(git_sdk(REV_A), &manifest, None);
            assert!(check_sdk_pin(tmp.path(), &config).is_err(), "{reference}");
        }
    }

    /// The consumer CI job patches the SDK to a local checkout through Cargo
    /// config, which rewrites the lockfile entry to a path.
    #[test]
    fn sdk_pin_accepts_a_lockfile_path_only_while_cargo_patches_it_there() {
        let manifest = format!(
            "[dependencies]\n{}",
            git_dep("petal", "bloom-petal-sdk", REV_A)
        );
        let patched =
            "version = 4\n\n[[package]]\nname = \"bloom-petal-sdk\"\nversion = \"0.1.0\"\n";
        let (tmp, config) = pin_fixture(git_sdk(REV_A), &manifest, Some(patched));
        check_sdk_pin_with(tmp.path(), &config, true, |manifest, package| {
            assert!(manifest.ends_with("route/Cargo.toml"), "{manifest:?}");
            assert_eq!(package, "bloom-petal-sdk");
            Ok(true)
        })
        .unwrap();
        // `--locked` also fails for a lockfile that is merely out of date, so
        // the error must not claim that no patch applies.
        let error = check_sdk_pin_with(tmp.path(), &config, true, |_, _| {
            Err("error: cannot update the lock file".into())
        })
        .unwrap_err();
        assert!(
            error.contains(
                "from a local path, and Cargo could not confirm that a config [patch] puts it there"
            ),
            "{error}"
        );
        assert!(error.contains("cannot update the lock file"), "{error}");

        // The release workflow refuses the patch without asking Cargo.
        let error = check_sdk_pin_with(tmp.path(), &config, false, |_, _| {
            panic!("asked Cargo although patches are refused")
        })
        .unwrap_err();
        assert!(
            error.contains("from a local path, which PETAL_REQUIRE_LOCKED_SOURCES=1 refuses"),
            "{error}"
        );
    }

    #[test]
    fn locked_sources_are_required_only_by_exactly_one() {
        assert_eq!(config_overrides_allowed(None), Ok(true));
        assert_eq!(config_overrides_allowed(Some("0".into())), Ok(true));
        assert_eq!(config_overrides_allowed(Some("1".into())), Ok(false));
        for value in ["", "true", "yes", "01"] {
            assert!(
                config_overrides_allowed(Some(value.into())).is_err(),
                "{value:?}"
            );
        }
    }

    #[test]
    fn sdk_pin_asks_cargo_only_about_a_lockfile_path() {
        let manifest = format!(
            "[dependencies]\n{}",
            git_dep("petal", "bloom-petal-sdk", REV_A)
        );
        let (tmp, config) = pin_fixture(git_sdk(REV_A), &manifest, Some(&git_lock(REV_A)));
        check_sdk_pin_with(tmp.path(), &config, true, |_, _| {
            panic!("asked Cargo about a lockfile that matches the pin")
        })
        .unwrap();
    }

    /// With `[sdk]` omitted, the route crate's dependency is the pin, and the
    /// error says which one it is.
    #[test]
    fn sdk_pin_errors_name_the_route_sdk_without_a_build_pin() {
        let manifest = format!(
            "[dependencies]\n{}",
            git_dep("petal", "bloom-petal-sdk", REV_A)
        );
        let (tmp, config) =
            pin_fixture(SdkDependency::default(), &manifest, Some(&git_lock(REV_B)));
        let error = check_sdk_pin(tmp.path(), &config).unwrap_err();
        assert!(
            error.contains(&format!(
                "pinned inconsistently with the route SDK git {SDK_GIT} rev {REV_A}"
            )),
            "{error}"
        );
    }

    /// Cargo applies a `[patch]` from Cargo config wherever it finds one, and
    /// only when it runs where it can find it.
    #[test]
    fn cargo_reports_a_patch_from_any_config_it_reads() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        // A local stand-in for the SDK repository, which Cargo reads to apply
        // a patch, and the checkout the patch points at.
        let origin = root.join("origin");
        let checkout = root.join("checkout");
        for crate_dir in [&origin, &checkout] {
            fs::create_dir_all(crate_dir.join("src")).unwrap();
            fs::write(
                crate_dir.join("Cargo.toml"),
                "[package]\nname = \"bloom-petal-sdk\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
            )
            .unwrap();
            fs::write(crate_dir.join("src/lib.rs"), "").unwrap();
        }
        let git = |args: &[&str]| {
            let output = Command::new("git")
                .current_dir(&origin)
                .args(["-c", "user.name=petal", "-c", "user.email=petal@invalid"])
                .args(args)
                .output()
                .unwrap();
            assert!(output.status.success(), "git {args:?}");
            String::from_utf8(output.stdout).unwrap()
        };
        git(&["init", "-q"]);
        git(&["add", "-A"]);
        git(&["commit", "-qm", "sdk"]);
        let rev = git(&["rev-parse", "HEAD"]);
        let url = format!("file://{}", origin.display());

        let petal = root.join("mono/petal");
        let manifest = petal.join("route/Cargo.toml");
        fs::create_dir_all(petal.join("route/src")).unwrap();
        fs::write(
            &manifest,
            format!(
                "[package]\nname = \"demo-route\"\nversion = \"0.1.0\"\nedition = \"2021\"\n[workspace]\n\n[dependencies]\npetal = {{ package = \"bloom-petal-sdk\", git = \"{url}\", rev = \"{}\" }}\n",
                rev.trim()
            ),
        )
        .unwrap();
        fs::write(petal.join("route/src/lib.rs"), "").unwrap();
        let home = root.join("cargo-home");
        fs::create_dir_all(&home).unwrap();
        // Colored output, as CI sets it, must not hide Cargo's error.
        let cargo = |cwd: &Path| {
            let mut cargo = Command::new("cargo");
            cargo
                .current_dir(cwd)
                .env("CARGO_HOME", &home)
                .env("CARGO_TERM_COLOR", "always");
            cargo
        };
        let patch = format!(
            "[patch.\"{url}\"]\nbloom-petal-sdk = {{ path = \"{}\" }}\n",
            checkout.display()
        );
        let resolves =
            |cwd: &Path| cargo_resolves_to_a_path(cargo(cwd), &manifest, "bloom-petal-sdk");

        // Lock the route workspace through an included config, as a developer
        // keeping the patch out of a committed file would.
        fs::create_dir_all(petal.join(".cargo")).unwrap();
        fs::write(petal.join(".cargo/local.toml"), &patch).unwrap();
        fs::write(
            petal.join(".cargo/config.toml"),
            "include = [\"local.toml\"]\n",
        )
        .unwrap();
        let status = cargo(&petal)
            .args(["generate-lockfile", "--quiet", "--manifest-path"])
            .arg(&manifest)
            .status()
            .unwrap();
        assert!(status.success());
        assert!(resolves(&petal).unwrap(), "through include");
        let outside = resolves(root).unwrap_err();
        assert!(outside.contains("--locked"), "{outside}");

        fs::remove_dir_all(petal.join(".cargo")).unwrap();
        fs::create_dir_all(root.join("mono/.cargo")).unwrap();
        fs::write(root.join("mono/.cargo/config.toml"), &patch).unwrap();
        assert!(resolves(&petal).unwrap(), "from a parent directory");

        fs::remove_dir_all(root.join("mono/.cargo")).unwrap();
        fs::write(home.join("config.toml"), &patch).unwrap();
        assert!(resolves(&petal).unwrap(), "from Cargo home");

        fs::remove_file(home.join("config.toml")).unwrap();
        assert!(resolves(&petal).is_err(), "with no patch");
    }

    #[test]
    fn sdk_pin_refuses_a_patch_or_replace_in_the_route_crate() {
        for overriding in [
            format!("[patch.\"{SDK_GIT}\"]\nbloom-petal-sdk = {{ path = \"../sdk\" }}\n"),
            "[replace]\n\"bloom-petal-sdk:0.1.0\" = { path = \"../sdk\" }\n".to_owned(),
            // Cargo also takes a package ID spec in URL form.
            format!("[replace]\n\"{SDK_GIT}#bloom-petal-sdk@0.1.0\" = {{ path = \"../sdk\" }}\n"),
            "[replace]\n\"file:///work/bloom-petal-sdk#0.1.0\" = { path = \"../sdk\" }\n"
                .to_owned(),
        ] {
            let manifest = format!(
                "[dependencies]\n{}\n{overriding}",
                git_dep("petal", "bloom-petal-sdk", REV_A)
            );
            let (tmp, config) = pin_fixture(git_sdk(REV_A), &manifest, None);
            let error = check_sdk_pin(tmp.path(), &config).unwrap_err();
            assert!(error.contains("overrides bloom-petal-sdk"), "{error}");
        }
    }

    #[test]
    fn package_id_specs_name_their_package_in_every_form() {
        for (spec, name) in [
            ("bloom-petal-sdk", "bloom-petal-sdk"),
            ("bloom-petal-sdk:0.1.0", "bloom-petal-sdk"),
            ("bloom-petal-sdk@0.1.0", "bloom-petal-sdk"),
            (
                "https://github.com/bloom-directory/petal#bloom-petal-sdk",
                "bloom-petal-sdk",
            ),
            (
                "https://github.com/bloom-directory/petal#bloom-petal-sdk@0.1.0",
                "bloom-petal-sdk",
            ),
            (
                "https://github.com/bloom-directory/petal#bloom-petal-sdk:0.1.0",
                "bloom-petal-sdk",
            ),
            (
                "git+https://github.com/bloom-directory/petal?rev=abc#bloom-petal-sdk",
                "bloom-petal-sdk",
            ),
            (
                "https://github.com/bloom-directory/bloom-petal-sdk",
                "bloom-petal-sdk",
            ),
            (
                "https://github.com/bloom-directory/bloom-petal-sdk/#0.1.0",
                "bloom-petal-sdk",
            ),
            ("file:///work/bloom-petal-sdk#0.1.0", "bloom-petal-sdk"),
        ] {
            assert_eq!(spec_package_name(spec), name, "{spec}");
        }
    }

    /// The lockfile records every `[replace]`, so a spelling the manifest
    /// check misses is still refused.
    #[test]
    fn sdk_pin_refuses_a_lockfile_entry_that_replace_rewrites() {
        let manifest = format!(
            "[dependencies]\n{}",
            git_dep("petal", "bloom-petal-sdk", REV_A)
        );
        let lock = git_lock(REV_A).replace(
            &format!("#{REV_A}\"\n"),
            &format!("#{REV_A}\"\nreplace = \"bloom-petal-sdk 0.1.0\"\n"),
        ) + "\n[[package]]\nname = \"bloom-petal-sdk\"\nversion = \"0.1.0\"\n";
        let (tmp, config) = pin_fixture(git_sdk(REV_A), &manifest, Some(&lock));
        let error = check_sdk_pin_with(tmp.path(), &config, true, |_, _| Ok(true)).unwrap_err();
        assert!(
            error.contains("replaces bloom-petal-sdk 0.1.0 with bloom-petal-sdk 0.1.0"),
            "{error}"
        );
    }

    /// Crates from the SDK's repository are held to the SDK's source in the
    /// lockfile and may not be overridden, as the SDK may not.
    #[test]
    fn crates_from_the_sdk_repository_are_held_like_the_sdk() {
        let manifest = format!(
            "[dependencies]\n{}\n[dev-dependencies]\n{}",
            git_dep("petal", "bloom-petal-sdk", REV_A),
            git_dep("bloom-petal-contract", "bloom-petal-contract", REV_A)
        );
        let contract = |fields: &str| {
            format!(
                "{}\n[[package]]\nname = \"bloom-petal-contract\"\nversion = \"0.1.0\"\n{fields}",
                git_lock(REV_A)
            )
        };
        let pinned = format!("source = \"git+{SDK_GIT}?rev={REV_A}#{REV_A}\"\n");
        let check = |lock: &str, allow_patch: bool, patched: bool| {
            let (tmp, config) = pin_fixture(git_sdk(REV_A), &manifest, Some(lock));
            check_sdk_pin_with(tmp.path(), &config, allow_patch, |_, package| {
                assert_eq!(package, "bloom-petal-contract");
                Ok(patched)
            })
        };
        check(&contract(&pinned), false, false).unwrap();
        let error = check(
            &contract("source = \"registry+https://github.com/rust-lang/crates.io-index\"\n"),
            true,
            true,
        )
        .unwrap_err();
        assert!(
            error.contains("resolves bloom-petal-contract 0.1.0 from registry+"),
            "{error}"
        );
        // A local path only through a patch Cargo confirms, and never in a release.
        check(&contract(""), true, true).unwrap();
        assert!(
            check(&contract(""), true, false)
                .unwrap_err()
                .contains("bloom-petal-contract 0.1.0 from a local path")
        );
        assert!(
            check(&contract(""), false, true)
                .unwrap_err()
                .contains("PETAL_REQUIRE_LOCKED_SOURCES=1 refuses")
        );
        let error = check(
            &contract(&format!(
                "{pinned}replace = \"bloom-petal-contract 0.1.0\"\n"
            )),
            true,
            true,
        )
        .unwrap_err();
        assert!(
            error.contains("replaces bloom-petal-contract 0.1.0"),
            "{error}"
        );

        let patched = format!(
            "{manifest}\n[patch.\"{SDK_GIT}\"]\nbloom-petal-contract = {{ path = \"../contract\" }}\n"
        );
        let (tmp, config) = pin_fixture(git_sdk(REV_A), &patched, None);
        let error = check_sdk_pin(tmp.path(), &config).unwrap_err();
        assert!(error.contains("overrides bloom-petal-contract"), "{error}");
    }

    #[test]
    fn sdk_pin_holds_other_crates_from_the_sdk_repository_to_the_same_rev() {
        // polymarket pins bloom-petal-contract from the SDK repository as a
        // dev-dependency; an SDK-only bump must not leave it behind.
        let manifest = format!(
            "[dependencies]\n{}\n[dev-dependencies]\n{}",
            git_dep("petal", "bloom-petal-sdk", REV_B),
            git_dep("bloom-petal-contract", "bloom-petal-contract", REV_A)
        );
        let (tmp, config) = pin_fixture(git_sdk(REV_B), &manifest, None);
        let error = check_sdk_pin(tmp.path(), &config).unwrap_err();
        assert!(error.contains("bloom-petal-contract"), "{error}");
        assert!(error.contains("same repository as the SDK"), "{error}");

        // The lockfile entry for that crate is held to the rev as well.
        let manifest = format!(
            "[dependencies]\n{}\n[dev-dependencies]\n{}",
            git_dep("petal", "bloom-petal-sdk", REV_B),
            git_dep("bloom-petal-contract", "bloom-petal-contract", REV_B)
        );
        let lock = format!(
            "{}\n[[package]]\nname = \"bloom-petal-contract\"\nversion = \"0.1.0\"\nsource = \"git+{SDK_GIT}?rev={REV_A}#{REV_A}\"\n",
            git_lock(REV_B)
        );
        let (tmp, config) = pin_fixture(git_sdk(REV_B), &manifest, Some(&lock));
        let error = check_sdk_pin(tmp.path(), &config).unwrap_err();
        assert!(error.contains("resolves bloom-petal-contract"), "{error}");

        // So is a crate from the SDK repository in the build configuration.
        let (tmp, mut config) = pin_fixture(
            git_sdk(REV_B),
            &format!(
                "[dependencies]\n{}",
                git_dep("petal", "bloom-petal-sdk", REV_B)
            ),
            None,
        );
        config.dependencies.insert(
            "bloom-petal-contract".into(),
            toml::from_str::<toml::Table>(&format!(
                "v = {{ git = \"{SDK_GIT}\", rev = \"{REV_A}\" }}"
            ))
            .unwrap()
            .remove("v")
            .unwrap(),
        );
        let error = check_sdk_pin(tmp.path(), &config).unwrap_err();
        assert!(error.contains("in the build configuration"), "{error}");
    }

    #[test]
    fn sdk_pin_resolves_workspace_inheritance_and_the_workspace_lockfile() {
        let workspace = |rev: &str| {
            format!(
                "[workspace]\nmembers = [\"route\"]\n\n[workspace.dependencies]\n{}",
                git_dep("petal", "bloom-petal-sdk", rev)
            )
        };
        // `petal.workspace = true` names the SDK only through the workspace.
        let (tmp, config) = pin_fixture(
            git_sdk(REV_A),
            "[dependencies]\npetal.workspace = true\n",
            None,
        );
        fs::write(tmp.path().join("Cargo.toml"), workspace(REV_B)).unwrap();
        let error = check_sdk_pin(tmp.path(), &config).unwrap_err();
        assert!(error.contains("route/Cargo.toml SDK"), "{error}");
        assert!(error.contains(&format!("rev {REV_B}")), "{error}");
        assert!(error.contains("remove or align [sdk]"), "{error}");

        // The lockfile lives at the workspace root and is checked there.
        fs::write(tmp.path().join("Cargo.toml"), workspace(REV_A)).unwrap();
        check_sdk_pin(tmp.path(), &config).unwrap();
        fs::write(tmp.path().join("Cargo.lock"), git_lock(REV_B)).unwrap();
        let error = check_sdk_pin(tmp.path(), &config).unwrap_err();
        assert!(
            error.contains("Cargo.lock resolves bloom-petal-sdk"),
            "{error}"
        );
    }

    /// Generated components always enable the SDK, so route tests must too.
    #[test]
    fn an_optional_sdk_is_refused_however_it_is_declared() {
        let direct = format!(
            "[dependencies]\npetal = {{ package = \"bloom-petal-sdk\", git = \"{SDK_GIT}\", rev = \"{REV_A}\", optional = true }}\n"
        );
        let (tmp, config) = pin_fixture(git_sdk(REV_A), &direct, None);
        let error = check_sdk_pin(tmp.path(), &config).unwrap_err();
        assert!(error.contains("must not be optional"), "{error}");

        // Cargo takes `optional` only from the inheriting entry.
        let (tmp, config) = pin_fixture(
            git_sdk(REV_A),
            "[dependencies]\npetal = { workspace = true, optional = true }\n",
            None,
        );
        fs::write(
            tmp.path().join("Cargo.toml"),
            format!(
                "[workspace]\nmembers = [\"route\"]\n[workspace.dependencies]\n{}",
                git_dep("petal", "bloom-petal-sdk", REV_A)
            ),
        )
        .unwrap();
        let error = check_sdk_pin(tmp.path(), &config).unwrap_err();
        assert!(error.contains("must not be optional"), "{error}");
    }

    /// Generated components ignore the route workspace's overrides of any
    /// crate, not only the SDK's, and `petal build` then refuses the changed
    /// graph; `check` and `package --no-build` must refuse it too.
    #[test]
    fn sdk_pin_refuses_any_patch_or_replace_in_the_route_workspace() {
        let manifest = format!(
            "[dependencies]\n{}",
            git_dep("petal", "bloom-petal-sdk", REV_A)
        );
        for overrides in [
            "[patch.crates-io]\nsha2 = { path = \"../sha2\" }\n",
            "[replace]\n\"sha2:0.11.0\" = { path = \"../sha2\" }\n",
        ] {
            let (tmp, config) =
                pin_fixture(git_sdk(REV_A), &format!("{manifest}{overrides}"), None);
            let error = check_sdk_pin(tmp.path(), &config).unwrap_err();
            assert!(
                error.contains(
                    "  - route/Cargo.toml [patch.\"crates-io\"] sha2 overrides sha2, which generated route components ignore"
                ) || error.contains(
                    "  - route/Cargo.toml [replace] \"sha2:0.11.0\" overrides sha2, which generated route components ignore"
                ),
                "{error}"
            );
            // The SDK pin itself is consistent, so the error says only this.
            assert!(!error.contains("pinned inconsistently"), "{error}");
            assert!(error.contains("Remove these overrides"), "{error}");

            let (tmp, config) = pin_fixture(git_sdk(REV_A), &manifest, None);
            fs::write(
                tmp.path().join("Cargo.toml"),
                format!("[workspace]\nmembers = [\"route\"]\n{overrides}"),
            )
            .unwrap();
            let error = check_sdk_pin(tmp.path(), &config).unwrap_err();
            assert!(error.contains("  - Cargo.toml ["), "{error}");
            assert!(
                error.contains("overrides sha2, which generated route components ignore"),
                "{error}"
            );
        }
    }

    #[test]
    fn sdk_pin_canonicalizes_urls_and_lockfile_revs_like_cargo() {
        // GitHub paths are case-insensitive and http means https.
        let manifest = format!(
            "[dependencies]\npetal = {{ package = \"bloom-petal-sdk\", git = \"http://github.com/Bloom-Directory/Petal\", rev = \"{REV_A}\" }}\n"
        );
        let (tmp, config) = pin_fixture(git_sdk(REV_A), &manifest, Some(&git_lock(REV_A)));
        check_sdk_pin(tmp.path(), &config).unwrap();

        // Lockfile v4 percent-encodes the rev; it is compared decoded.
        let encoded = git_lock(REV_A).replace(
            &format!("?rev={REV_A}"),
            &format!("?rev=%{:02X}{}", REV_A.as_bytes()[0], &REV_A[1..]),
        );
        let manifest = format!(
            "[dependencies]\n{}",
            git_dep("petal", "bloom-petal-sdk", REV_A)
        );
        let (tmp, config) = pin_fixture(git_sdk(REV_A), &manifest, Some(&encoded));
        check_sdk_pin(tmp.path(), &config).unwrap();
    }

    #[test]
    fn sdk_pin_refuses_an_alternate_registry_and_checks_old_table_names() {
        let (tmp, config) = pin_fixture(
            package_config().sdk,
            "[dependencies]\npetal = { package = \"bloom-petal-sdk\", version = \"=0.1.0\", registry = \"mirror\" }\n",
            None,
        );
        let error = check_sdk_pin(tmp.path(), &config).unwrap_err();
        assert!(error.contains("from registry mirror"), "{error}");

        let (tmp, config) = pin_fixture(
            package_config().sdk,
            "[dependencies]\npetal = { package = \"bloom-petal-sdk\", version = \"=0.1.0\" }\n[dev_dependencies]\npetal = { package = \"bloom-petal-sdk\", version = \"=0.2.0\" }\n",
            None,
        );
        let error = check_sdk_pin(tmp.path(), &config).unwrap_err();
        assert!(error.contains("[dev_dependencies] petal"), "{error}");
    }

    #[test]
    fn sdk_config_requires_an_exact_pin() {
        let mut sdk = git_sdk(REV_A);
        validate_sdk(&sdk).unwrap();
        sdk.rev = None;
        assert!(
            validate_sdk(&sdk)
                .unwrap_err()
                .contains("40-character commit")
        );
        sdk.rev = Some("2225574".into());
        assert!(validate_sdk(&sdk).is_err());

        for version in ["=0.1.0", "= 0.1.0", "=1.2.3-rc.1"] {
            let mut sdk = package_config().sdk;
            sdk.version = Some(version.into());
            validate_sdk(&sdk).unwrap_or_else(|error| panic!("{version}: {error}"));
        }
        for version in ["0.1.0", "^0.1.0", "~0.1", "=0.1", ">=0.1.0, <0.2.0", "*"] {
            let mut sdk = package_config().sdk;
            sdk.version = Some(version.into());
            assert!(validate_sdk(&sdk).is_err(), "{version}");
        }
    }

    #[test]
    fn sdk_pin_ignores_trailing_slash_and_dot_git_in_urls() {
        let manifest = format!(
            "[dependencies]\npetal = {{ package = \"bloom-petal-sdk\", git = \"{SDK_GIT}.git/\", rev = \"{REV_A}\" }}\n"
        );
        let (tmp, config) = pin_fixture(git_sdk(REV_A), &manifest, Some(&git_lock(REV_A)));
        check_sdk_pin(tmp.path(), &config).unwrap();
    }

    #[test]
    fn sdk_pin_checks_every_dependency_table() {
        for section in [
            "dev-dependencies",
            "build-dependencies",
            "target.'cfg(target_arch = \"wasm32\")'.dependencies",
        ] {
            let manifest = format!(
                "[dependencies]\npetal = {{ package = \"bloom-petal-sdk\", git = \"{SDK_GIT}\", rev = \"{REV_A}\" }}\n\n[{section}]\nsdk = {{ package = \"bloom-petal-sdk\", git = \"{SDK_GIT}\", rev = \"{REV_B}\" }}\n"
            );
            let (tmp, config) = pin_fixture(git_sdk(REV_A), &manifest, None);
            let error = check_sdk_pin(tmp.path(), &config).unwrap_err();
            assert!(
                error.contains(&format!("rev {REV_B}")),
                "{section}: {error}"
            );
            assert!(
                !error.contains("[dependencies] petal"),
                "{section}: {error}"
            );
        }
    }

    #[test]
    fn sdk_pin_recognizes_the_package_by_name_or_rename() {
        // A plain `bloom-petal-sdk = "..."` entry is the SDK too.
        let (tmp, config) = pin_fixture(
            package_config().sdk,
            "[dependencies]\nbloom-petal-sdk = \"=0.2.0\"\n",
            None,
        );
        let error = check_sdk_pin(tmp.path(), &config).unwrap_err();
        assert!(
            error.contains("route/Cargo.toml SDK version =0.2.0"),
            "{error}"
        );

        // Other packages, including one renamed to `petal`, are not.
        let (tmp, config) = pin_fixture(
            package_config().sdk,
            "[dependencies]\npetal = { package = \"other\", version = \"9\" }\nserde = \"1\"\n",
            None,
        );
        assert!(
            check_sdk_pin(tmp.path(), &config)
                .unwrap_err()
                .contains("exactly one")
        );
    }

    #[test]
    fn sdk_pin_compares_versions_exactly() {
        let (tmp, config) = pin_fixture(
            package_config().sdk,
            "[dependencies]\npetal = { package = \"bloom-petal-sdk\", version = \"=0.1.0\" }\n",
            Some(
                "version = 4\n\n[[package]]\nname = \"bloom-petal-sdk\"\nversion = \"0.1.0\"\nsource = \"registry+https://github.com/rust-lang/crates.io-index\"\n",
            ),
        );
        check_sdk_pin(tmp.path(), &config).unwrap();

        // Whitespace inside a requirement does not change it.
        let (tmp, config) = pin_fixture(
            package_config().sdk,
            "[dependencies]\npetal = { package = \"bloom-petal-sdk\", version = \"= 0.1.0\" }\n",
            None,
        );
        check_sdk_pin(tmp.path(), &config).unwrap();

        // `0.1.0` is a caret range, not the exact `=0.1.0` that [sdk] pins.
        let (tmp, config) = pin_fixture(
            package_config().sdk,
            "[dependencies]\npetal = { package = \"bloom-petal-sdk\", version = \"0.1.0\" }\n",
            None,
        );
        assert!(check_sdk_pin(tmp.path(), &config).is_err());

        // An exact version pin must also be what the lockfile resolved.
        let (tmp, config) = pin_fixture(
            package_config().sdk,
            "[dependencies]\npetal = { package = \"bloom-petal-sdk\", version = \"=0.1.0\" }\n",
            Some(
                "version = 4\n\n[[package]]\nname = \"bloom-petal-sdk\"\nversion = \"0.1.1\"\nsource = \"registry+https://github.com/rust-lang/crates.io-index\"\n",
            ),
        );
        assert!(
            check_sdk_pin(tmp.path(), &config)
                .unwrap_err()
                .contains("0.1.1")
        );
    }

    #[test]
    fn sdk_pin_resolves_paths_from_their_own_manifests() {
        let (tmp, mut config) = pin_fixture(
            package_config().sdk,
            "[dependencies]\npetal = { package = \"bloom-petal-sdk\", path = \"../sdk\" }\n",
            Some("version = 4\n\n[[package]]\nname = \"bloom-petal-sdk\"\nversion = \"0.1.0\"\n"),
        );
        fs::create_dir_all(tmp.path().join("sdk")).unwrap();
        fs::create_dir_all(tmp.path().join("other-sdk")).unwrap();
        config.sdk = SdkDependency {
            package: "bloom-petal-sdk".into(),
            version: None,
            path: Some("sdk".into()),
            git: None,
            rev: None,
        };
        check_sdk_pin(tmp.path(), &config).unwrap();

        config.sdk.path = Some("other-sdk".into());
        let error = check_sdk_pin(tmp.path(), &config).unwrap_err();
        assert!(error.contains("other-sdk"), "{error}");
    }

    #[test]
    fn sdk_pin_requires_a_route_crate_sdk_source() {
        let (tmp, config) = pin_fixture(git_sdk(REV_A), "[dependencies]\nserde = \"1\"\n", None);
        assert!(
            check_sdk_pin(tmp.path(), &config)
                .unwrap_err()
                .contains("exactly one")
        );
    }

    #[test]
    fn package_refuses_disagreeing_sdk_pins() {
        let tmp = tempfile::tempdir().unwrap();
        package_fixture(tmp.path());
        fs::create_dir_all(tmp.path().join("route")).unwrap();
        fs::write(
            tmp.path().join("route/Cargo.toml"),
            "[package]\nname = \"demo-route\"\nversion = \"0.1.0\"\nedition = \"2024\"\n\n[dependencies]\npetal = { package = \"bloom-petal-sdk\", version = \"=0.2.0\" }\n",
        )
        .unwrap();
        let error = package(
            tmp.path(),
            &package_config(),
            tmp.path().join("out/demo.petal.tar.gz"),
        )
        .unwrap_err();
        assert!(error.contains("pinned inconsistently"), "{error}");
        assert!(!tmp.path().join("out/demo.petal.tar.gz").exists());
    }

    fn package_fixture(root: &Path) {
        fs::create_dir_all(root.join("petal/demo")).unwrap();
        fs::write(
            root.join("petal.toml"),
            "schema = \"bloom.petal.package.v1\"\nname = \"demo\"\n",
        )
        .unwrap();
        fs::write(root.join("README.md"), "demo\n").unwrap();
        fs::write(root.join("AGENTS.md"), "agent guidance\n").unwrap();
        fs::write(root.join("petal/demo/status.wasm"), b"component").unwrap();
        fs::create_dir_all(root.join("route")).unwrap();
        fs::write(
            root.join("route/Cargo.toml"),
            "[package]\nname = \"demo-route\"\nversion = \"0.1.0\"\nedition = \"2024\"\n\n[dependencies]\npetal = { package = \"bloom-petal-sdk\", version = \"=0.1.0\" }\n",
        )
        .unwrap();
    }

    #[test]
    fn sha256_matches_known_vectors() {
        assert_eq!(
            hex_sha256(b""),
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
        assert_eq!(
            hex_sha256(b"abc"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }

    #[test]
    fn runtime_identity_ignores_development_reports_but_binds_code_and_capabilities() {
        let fixture = tempfile::tempdir().unwrap();
        package_fixture(fixture.path());
        let archive = |name: &str| fixture.path().join(format!("dist/{name}.petal.tar.gz"));
        let original = package(fixture.path(), &package_config(), archive("original")).unwrap();
        fs::write(fixture.path().join("audit-notes.md"), b"development notes").unwrap();
        fs::write(fixture.path().join("route/report.txt"), b"not runtime").unwrap();
        let reported = package(fixture.path(), &package_config(), archive("reported")).unwrap();
        assert_eq!(original.package_hash, reported.package_hash);
        assert_eq!(original.sha256, reported.sha256);
        fs::write(
            fixture.path().join("petal/demo/status.wasm"),
            b"changed code",
        )
        .unwrap();
        let code = package(fixture.path(), &package_config(), archive("code")).unwrap();
        assert_ne!(original.package_hash, code.package_hash);
        let manifest = fixture.path().join("petal.toml");
        fs::write(
            &manifest,
            format!(
                "{}\n[caps]\nallowed = [\"bloom:sign\"]\n",
                fs::read_to_string(&manifest).unwrap()
            ),
        )
        .unwrap();
        let capabilities =
            package(fixture.path(), &package_config(), archive("capabilities")).unwrap();
        assert_ne!(code.package_hash, capabilities.package_hash);
    }

    #[test]
    fn package_keeps_the_scaffolded_skill_as_runtime_identity() {
        let fixture = tempfile::tempdir().unwrap();
        package_fixture(fixture.path());
        let archive = |name: &str| fixture.path().join(format!("dist/{name}.petal.tar.gz"));
        let without = package(fixture.path(), &package_config(), archive("without")).unwrap();
        // A SKILL.md at the root is a development document, not the scaffold's.
        fs::write(fixture.path().join("SKILL.md"), "notes\n").unwrap();
        let root_document = package(fixture.path(), &package_config(), archive("root")).unwrap();
        assert_eq!(without.package_hash, root_document.package_hash);

        let skill = fixture
            .path()
            .join(bloom_petal_contract::PETAL_DEVELOPMENT_SKILL_PATH);
        fs::create_dir_all(skill.parent().unwrap()).unwrap();
        fs::write(&skill, "skill v1\n").unwrap();
        let first = package(fixture.path(), &package_config(), archive("first")).unwrap();
        assert_ne!(without.package_hash, first.package_hash);
        let decoder = flate2::read::GzDecoder::new(fs::File::open(archive("first")).unwrap());
        let entries = tar::Archive::new(decoder)
            .entries()
            .unwrap()
            .map(|entry| {
                entry
                    .unwrap()
                    .path()
                    .unwrap()
                    .to_string_lossy()
                    .into_owned()
            })
            .collect::<Vec<_>>();
        assert!(
            entries
                .iter()
                .any(|path| path == bloom_petal_contract::PETAL_DEVELOPMENT_SKILL_PATH),
            "{entries:?}"
        );
        assert!(
            !entries.iter().any(|path| path == "SKILL.md"),
            "{entries:?}"
        );

        fs::write(&skill, "skill v2, edited\n").unwrap();
        let edited = package(fixture.path(), &package_config(), archive("edited")).unwrap();
        assert_ne!(first.package_hash, edited.package_hash);
    }

    #[cfg(unix)]
    #[test]
    fn package_refuses_a_skill_reached_through_a_linked_directory() {
        let fixture = tempfile::tempdir().unwrap();
        package_fixture(fixture.path());
        let outside = tempfile::tempdir().unwrap();
        let skill = Path::new(bloom_petal_contract::PETAL_DEVELOPMENT_SKILL_PATH);
        let target = outside.path().join(skill.strip_prefix(".agents").unwrap());
        fs::create_dir_all(target.parent().unwrap()).unwrap();
        fs::write(&target, "outside the Petal\n").unwrap();
        std::os::unix::fs::symlink(outside.path(), fixture.path().join(".agents")).unwrap();
        let out = fixture.path().join("dist/linked.petal.tar.gz");
        let error = package(fixture.path(), &package_config(), &out).unwrap_err();
        assert!(error.contains("crosses non-directory"), "{error}");
        assert!(!out.exists());
    }

    #[test]
    fn package_refuses_a_skill_that_is_not_a_regular_file() {
        let fixture = tempfile::tempdir().unwrap();
        package_fixture(fixture.path());
        let skill = fixture
            .path()
            .join(bloom_petal_contract::PETAL_DEVELOPMENT_SKILL_PATH);
        fs::create_dir_all(&skill).unwrap();
        fs::write(skill.join("notes.txt"), "not the skill\n").unwrap();
        let out = fixture.path().join("dist/directory.petal.tar.gz");
        let error = package(fixture.path(), &package_config(), &out).unwrap_err();
        assert!(error.contains("is not a regular file"), "{error}");
        assert!(!out.exists());
    }

    #[test]
    fn package_takes_the_skill_only_under_its_exact_name() {
        let fixture = tempfile::tempdir().unwrap();
        package_fixture(fixture.path());
        let archive = |name: &str| fixture.path().join(format!("dist/{name}.petal.tar.gz"));
        let without = package(fixture.path(), &package_config(), archive("without")).unwrap();
        // A case-insensitive file system finds this under the skill's path; a
        // case-sensitive one does not, and both must package the same tree.
        let skill = fixture
            .path()
            .join(".Agents/Skills/Bloom-Petal-Development/skill.md");
        fs::create_dir_all(skill.parent().unwrap()).unwrap();
        fs::write(&skill, "skill\n").unwrap();
        let cased = package(fixture.path(), &package_config(), archive("cased")).unwrap();
        assert_eq!(without.package_hash, cased.package_hash);
    }

    /// Holds on any file system, unlike the packaging test above, which only
    /// a case-insensitive one exercises.
    #[test]
    fn entries_match_only_their_exact_name() {
        let directory = tempfile::tempdir().unwrap();
        fs::write(directory.path().join("Skill.md"), "skill\n").unwrap();
        assert!(has_entry(directory.path(), OsStr::new("Skill.md")).unwrap());
        assert!(!has_entry(directory.path(), OsStr::new("skill.md")).unwrap());
        assert!(!has_entry(directory.path(), OsStr::new("SKILL.md")).unwrap());
    }

    #[test]
    fn component_flags_follow_the_developers_build_rustflags() {
        let flags = ["--remap-path-prefix=/root=/petal", "-Cdebuginfo=0"];
        assert_eq!(
            appended_build_rustflags(None, &flags).unwrap(),
            "--remap-path-prefix=/root=/petal -Cdebuginfo=0"
        );
        assert_eq!(
            appended_build_rustflags(Some(" --cfg developer  -Copt-level=3 "), &flags).unwrap(),
            "--cfg developer  -Copt-level=3 --remap-path-prefix=/root=/petal -Cdebuginfo=0"
        );
        assert_eq!(
            appended_build_rustflags(Some("  "), &flags).unwrap(),
            "--remap-path-prefix=/root=/petal -Cdebuginfo=0"
        );
        // Cargo splits the variable on whitespace, so such a path needs an array.
        assert_eq!(
            appended_build_rustflags(None, &["--remap-path-prefix=/my root=/petal"]),
            None
        );
    }

    /// Cargo cannot merge that array into a string `build.rustflags` from
    /// Cargo config, so the build stops first and says why.
    #[test]
    fn a_rustflags_array_against_a_configured_string_says_why() {
        let tmp = tempfile::tempdir().unwrap();
        fs::create_dir_all(tmp.path().join("src")).unwrap();
        fs::create_dir_all(tmp.path().join(".cargo")).unwrap();
        fs::write(
            tmp.path().join("Cargo.toml"),
            "[package]\nname = \"probe\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
        )
        .unwrap();
        fs::write(tmp.path().join("src/lib.rs"), "").unwrap();
        let flags = ["--remap-path-prefix=/my root=/petal"];
        let setting = |config: &str| {
            fs::write(tmp.path().join(".cargo/config.toml"), config).unwrap();
            let mut cargo = Command::new("cargo");
            cargo.current_dir(tmp.path());
            rustflags_array_setting(cargo, &tmp.path().join("Cargo.toml"), &flags)
        };
        let error = setting("[build]\nrustflags = \"--cfg developer\"\n").unwrap_err();
        assert!(error.contains("path contains whitespace"), "{error}");
        assert!(error.contains("Set build.rustflags as an array"), "{error}");
        assert_eq!(
            setting("[build]\nrustflags = [\"--cfg\", \"developer\"]\n").unwrap(),
            "build.rustflags=[\"--remap-path-prefix=/my root=/petal\"]"
        );
    }

    #[test]
    fn toolchain_sources_map_to_the_path_rustc_embeds_without_rust_src() {
        let Some((source, embedded)) = toolchain_source().unwrap() else {
            return;
        };
        assert!(source.ends_with("lib/rustlib/src/rust"), "{source:?}");
        let commit = embedded.strip_prefix("/rustc/").unwrap();
        assert!(is_full_commit(commit), "{embedded}");
    }

    #[test]
    fn components_are_found_by_the_name_bloom_installs_as_a_route() {
        let output = tempfile::tempdir().unwrap();
        fs::create_dir_all(output.path().join("sub")).unwrap();
        for name in [
            "status.wasm",
            ".wasm",
            "sub/.wasm",
            "sub/plan.md.wasm",
            "notes.txt",
        ] {
            fs::write(output.path().join(name), b"component").unwrap();
        }
        assert_eq!(
            discover_wasm_paths(output.path()).unwrap(),
            [".wasm", "status.wasm", "sub/.wasm", "sub/plan.md.wasm"]
                .into_iter()
                .map(String::from)
                .collect::<BTreeSet<_>>()
        );
    }

    #[test]
    fn package_archive_is_deterministic_and_strict() {
        let fixture = tempfile::tempdir().unwrap();
        package_fixture(fixture.path());
        let first = fixture.path().join("dist/demo-a.petal.tar.gz");
        let second = fixture.path().join("dist/demo-b.petal.tar.gz");
        let first_report = package(fixture.path(), &package_config(), &first).unwrap();
        let second_report = package(fixture.path(), &package_config(), &second).unwrap();
        assert_eq!(fs::read(&first).unwrap(), fs::read(&second).unwrap());
        assert_eq!(first_report.package_hash, second_report.package_hash);
        assert_eq!(first_report.sha256, second_report.sha256);
        assert_eq!(first_report.routes, 1);

        let decoder = flate2::read::GzDecoder::new(fs::File::open(first).unwrap());
        let mut archive = tar::Archive::new(decoder);
        let entries = archive
            .entries()
            .unwrap()
            .map(|entry| {
                let entry = entry.unwrap();
                assert_eq!(entry.header().mode().unwrap(), 0o644);
                assert_eq!(entry.header().uid().unwrap(), 0);
                assert_eq!(entry.header().gid().unwrap(), 0);
                assert_eq!(entry.header().mtime().unwrap(), 0);
                entry.path().unwrap().to_string_lossy().into_owned()
            })
            .collect::<Vec<_>>();
        assert!(entries.contains(&"petal.toml".into()));
        assert!(entries.contains(&"petal/demo/status.wasm".into()));
        assert!(!entries.iter().any(|path| path.ends_with(".petal.tar.gz")));
    }

    #[test]
    fn package_requires_manifest_identity_and_docs() {
        let fixture = tempfile::tempdir().unwrap();
        package_fixture(fixture.path());
        fs::remove_file(fixture.path().join("AGENTS.md")).unwrap();
        let error = package(
            fixture.path(),
            &package_config(),
            fixture.path().join("demo.petal.tar.gz"),
        )
        .unwrap_err();
        assert!(error.contains("missing required file AGENTS.md"));
    }

    fn archive_entries(archive: &Path) -> Vec<String> {
        let decoder = flate2::read::GzDecoder::new(fs::File::open(archive).unwrap());
        tar::Archive::new(decoder)
            .entries()
            .unwrap()
            .map(|entry| {
                entry
                    .unwrap()
                    .path()
                    .unwrap()
                    .to_string_lossy()
                    .into_owned()
            })
            .collect()
    }

    /// Bloom composes route sidecars from these package directories and
    /// refuses any entry under petal/ other than petal/<name>/.
    #[test]
    fn package_holds_what_bloom_installs_and_nothing_it_refuses() {
        let fixture = tempfile::tempdir().unwrap();
        package_fixture(fixture.path());
        for (path, body) in [
            (
                "petal/demo/status.json.route.toml",
                "abi = \"route\"\ncomponent = \"components/status.wasm\"\n",
            ),
            ("components/status.wasm", "component"),
            ("modules/helper.wasm", "module"),
            ("wit/world.wit", "package demo:route;\n"),
            ("petal/.DS_Store", "finder"),
            ("petal/old-name/status.wasm", "a renamed Petal's output"),
        ] {
            let path = fixture.path().join(path);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(path, body).unwrap();
        }
        let out = fixture.path().join("dist/demo.petal.tar.gz");
        package(fixture.path(), &package_config(), &out).unwrap();
        let entries = archive_entries(&out);
        for expected in [
            "petal/demo/status.json.route.toml",
            "components/status.wasm",
            "modules/helper.wasm",
            "wit/world.wit",
        ] {
            assert!(
                entries.iter().any(|path| path == expected),
                "{expected}: {entries:?}"
            );
        }
        assert!(
            !entries
                .iter()
                .any(|path| path.starts_with("petal/") && !path.starts_with("petal/demo/")),
            "{entries:?}"
        );
    }

    /// A case-insensitive file system finds README.md under another case, and
    /// a case-sensitive one does not; both must refuse the tree.
    #[test]
    fn package_requires_its_documents_under_their_exact_names() {
        let fixture = tempfile::tempdir().unwrap();
        package_fixture(fixture.path());
        fs::rename(
            fixture.path().join("README.md"),
            fixture.path().join("readme.md"),
        )
        .unwrap();
        let error = package(
            fixture.path(),
            &package_config(),
            fixture.path().join("dist/demo.petal.tar.gz"),
        )
        .unwrap_err();
        assert!(error.contains("missing required file README.md"), "{error}");
    }

    #[cfg(unix)]
    #[test]
    fn package_refuses_links_in_the_package_trees() {
        for (link, to_directory, refusal) in [
            ("petal/demo/linked.wasm", false, "is a link"),
            ("petal/demo/linked", true, "is a link"),
            ("components", true, "non-regular file"),
            ("petal", true, "crosses non-directory"),
        ] {
            let fixture = tempfile::tempdir().unwrap();
            package_fixture(fixture.path());
            let outside = tempfile::tempdir().unwrap();
            fs::create_dir_all(outside.path().join("demo")).unwrap();
            fs::write(outside.path().join("demo/status.wasm"), b"outside").unwrap();
            let target = if to_directory {
                outside.path().to_owned()
            } else {
                outside.path().join("demo/status.wasm")
            };
            let link_path = fixture.path().join(link);
            if link == "petal" {
                fs::rename(&link_path, fixture.path().join("moved-petal")).unwrap();
            }
            std::os::unix::fs::symlink(&target, &link_path).unwrap();
            let out = fixture.path().join("dist/linked.petal.tar.gz");
            let error = package(fixture.path(), &package_config(), &out).unwrap_err();
            assert!(error.contains(refusal), "{link}: {error}");
            assert!(!out.exists(), "{link}");
        }
    }

    #[test]
    fn package_requires_its_documents_to_be_regular_files() {
        let fixture = tempfile::tempdir().unwrap();
        package_fixture(fixture.path());
        fs::remove_file(fixture.path().join("README.md")).unwrap();
        fs::create_dir(fixture.path().join("README.md")).unwrap();
        fs::write(
            fixture.path().join("README.md/notes.md"),
            "not the readme\n",
        )
        .unwrap();
        let out = fixture.path().join("dist/demo.petal.tar.gz");
        let error = package(fixture.path(), &package_config(), &out).unwrap_err();
        assert!(error.contains("README.md is not a regular file"), "{error}");
        assert!(!out.exists());
    }

    /// A case-insensitive file system finds `Components/` as `components/`; a
    /// case-sensitive one does not, and both must package the same tree.
    #[test]
    fn package_takes_optional_directories_only_under_their_exact_names() {
        let fixture = tempfile::tempdir().unwrap();
        package_fixture(fixture.path());
        fs::create_dir(fixture.path().join("Components")).unwrap();
        fs::write(fixture.path().join("Components/status.wasm"), b"component").unwrap();
        let out = fixture.path().join("dist/demo.petal.tar.gz");
        package(fixture.path(), &package_config(), &out).unwrap();
        let entries = archive_entries(&out);
        assert!(
            !entries
                .iter()
                .any(|path| path.to_ascii_lowercase().starts_with("components/")),
            "{entries:?}"
        );
    }

    #[test]
    fn package_refuses_a_file_in_place_of_an_optional_directory() {
        for directory in OPTIONAL_PACKAGE_DIRECTORIES {
            let fixture = tempfile::tempdir().unwrap();
            package_fixture(fixture.path());
            fs::write(fixture.path().join(directory), b"not a directory").unwrap();
            let out = fixture.path().join("dist/demo.petal.tar.gz");
            let error = package(fixture.path(), &package_config(), &out).unwrap_err();
            assert!(
                error.contains(&format!("{directory} must be a directory")),
                "{error}"
            );
            assert!(!out.exists(), "{directory}");
        }
    }

    /// A later package would take an archive written into a packaged tree.
    #[test]
    fn package_refuses_to_write_its_archive_into_a_packaged_tree() {
        let fixture = tempfile::tempdir().unwrap();
        package_fixture(fixture.path());
        for directory in PACKAGED_DIRECTORIES {
            fs::create_dir_all(fixture.path().join(directory)).unwrap();
            // The fixture's own path is not resolved: macOS keeps temporary
            // files behind /var. `..` after a directory that does not exist
            // yet resolves once it is created.
            let mut outputs = vec![
                directory.to_owned(),
                format!("not-yet/../{directory}"),
                format!("dist/../{directory}/nested"),
            ];
            // A case-insensitive file system writes `PETAL/x` into `petal/`;
            // a case-sensitive one keeps them apart.
            if fixture.path().join(directory.to_uppercase()).exists() {
                outputs.push(directory.to_uppercase());
                outputs.push(format!("not-yet/../{}", directory.to_uppercase()));
            }
            for named in outputs {
                let out = fixture.path().join(&named).join("demo.petal.tar.gz");
                let error = package(fixture.path(), &package_config(), &out).unwrap_err();
                assert!(
                    error.contains(&format!("outside the packaged {directory}/ tree")),
                    "{named}: {error}"
                );
                assert!(!out.exists(), "{named}");
            }
        }
        // A link that only `..` past a missing directory reaches.
        #[cfg(unix)]
        {
            std::os::unix::fs::symlink(
                fixture.path().join("petal/demo"),
                fixture.path().join("dist-link"),
            )
            .unwrap();
            let out = fixture
                .path()
                .join("not-yet/../dist-link/demo.petal.tar.gz");
            let error = package(fixture.path(), &package_config(), &out).unwrap_err();
            assert!(
                error.contains("outside the packaged petal/ tree"),
                "{error}"
            );
            fs::remove_file(fixture.path().join("dist-link")).unwrap();
        }
        // An archive left in a packaged tree is not packaged.
        for archive in ["components/old.petal.tar.gz", "modules/old.petal.tar"] {
            fs::write(fixture.path().join(archive), b"archive").unwrap();
        }
        let out = fixture.path().join("dist/demo.petal.tar.gz");
        package(fixture.path(), &package_config(), &out).unwrap();
        let entries = archive_entries(&out);
        assert!(
            !entries
                .iter()
                .any(|path| path.ends_with(".petal.tar.gz") || path.ends_with(".petal.tar")),
            "{entries:?}"
        );
    }

    #[test]
    fn paths_resolve_lexically_after_what_exists() {
        assert_eq!(
            lexical_normalize(Path::new("/a/b/../c/./d/..")),
            Path::new("/a/c")
        );
        assert_eq!(
            lexical_normalize(Path::new("../../x/..")),
            Path::new("../..")
        );
        assert_eq!(lexical_normalize(Path::new("/../a")), Path::new("/a"));
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().canonicalize().unwrap();
        assert_eq!(
            resolve_existing(&tmp.path().join("missing/../petal/x")),
            root.join("petal/x")
        );
    }

    #[cfg(unix)]
    #[test]
    fn component_discovery_refuses_links_instead_of_walking_them() {
        let fixture = tempfile::tempdir().unwrap();
        package_fixture(fixture.path());
        std::os::unix::fs::symlink(".", fixture.path().join("petal/demo/loop")).unwrap();
        let error = discover_wasm_paths(&fixture.path().join("petal/demo")).unwrap_err();
        assert!(
            error
                .ends_with("loop is a link; a Petal runtime tree holds only files and directories"),
            "{error}"
        );
        // A linked root is refused even when its target holds no link.
        let plain = tempfile::tempdir().unwrap();
        fs::write(plain.path().join("status.wasm"), b"component").unwrap();
        std::os::unix::fs::symlink(plain.path(), fixture.path().join("linked")).unwrap();
        assert!(
            discover_wasm_paths(&fixture.path().join("linked"))
                .unwrap_err()
                .contains("is a link")
        );
    }

    /// Generated components ignore `[replace]`, so a replaced crate would run
    /// in the route's tests but not in the components, whichever crate it is.
    #[test]
    fn sdk_pin_refuses_a_lockfile_that_replaces_any_crate() {
        let manifest = format!(
            "[dependencies]\n{}hex = \"0.4\"\n",
            git_dep("petal", "bloom-petal-sdk", REV_A)
        );
        let lock = format!(
            "{}\n[[package]]\nname = \"hex\"\nversion = \"0.4.3\"\nsource = \"registry+https://github.com/rust-lang/crates.io-index\"\nreplace = \"hex 0.4.3\"\n\n[[package]]\nname = \"hex\"\nversion = \"0.4.3\"\n",
            git_lock(REV_A)
        );
        let (tmp, config) = pin_fixture(git_sdk(REV_A), &manifest, Some(&lock));
        let error = check_sdk_pin_with(tmp.path(), &config, true, |_, _| Ok(true)).unwrap_err();
        assert!(
            error.contains("replaces hex 0.4.3 with hex 0.4.3"),
            "{error}"
        );
    }

    #[test]
    fn release_builds_use_only_what_the_tested_lockfile_names() {
        let tmp = tempfile::tempdir().unwrap();
        let home = tmp.path().join("cargo-home");
        let checkout = tmp.path().join("checkout");
        let generated = checkout.join("target/petal-routes/workspace");
        let git = format!("git+{SDK_GIT}?rev={REV_A}#{REV_A}");
        // Cargo percent-encodes the branch in the lockfile, `%` included, and
        // shows it as written in metadata.
        let branch_locked = format!("git+{SDK_GIT}?branch=feat%2Fx%2541#{REV_A}");
        let branch_shown = format!("git+{SDK_GIT}?branch=feat/x%41#{REV_A}");
        let registry = "registry+https://github.com/rust-lang/crates.io-index";
        let tested: toml::Table = toml::from_str(&format!(
            "version = 4\n[[package]]\nname = \"bloom-petal-sdk\"\nversion = \"0.1.0\"\nsource = \"{git}\"\n[[package]]\nname = \"hex\"\nversion = \"0.4.3\"\nsource = \"{registry}\"\n[[package]]\nname = \"helper\"\nversion = \"0.1.0\"\nsource = \"{branch_locked}\"\n[[package]]\nname = \"demo-route\"\nversion = \"0.1.0\"\n"
        ))
        .unwrap();
        let package = |name: &str, version: &str, source: Option<&str>, manifest: PathBuf| serde_json::json!({"name": name, "version": version, "source": source, "manifest_path": manifest});
        let sdk_checkout =
            home.join("git/checkouts/petal-0123456789abcdef/2225574/crates/petal-sdk/Cargo.toml");
        let check = |sdk: serde_json::Value| {
            let metadata = serde_json::json!({"packages": [
                sdk,
                package("hex", "0.4.3", Some(registry), home.join("registry/src/index.crates.io-1949cf8c6b5b557f/hex-0.4.3/Cargo.toml")),
                package("helper", "0.1.0", Some(&branch_shown), home.join("git/checkouts/helper-0123456789abcdef/2225574/Cargo.toml")),
                package("demo-route", "0.1.0", None, checkout.join("route/Cargo.toml")),
                package("petal-route-status", "0.1.0", None, generated.join("members/petal-route-status/Cargo.toml")),
            ]});
            require_locked_sources(
                &tested,
                &metadata,
                &home,
                &generated,
                &BTreeSet::from(["petal-route-status"]),
                &BTreeSet::from([checkout.join("route")]),
            )
        };
        // Cargo's own copies, with a percent-encoded branch, and the
        // checkout's own path packages.
        check(package(
            "bloom-petal-sdk",
            "0.1.0",
            Some(&git),
            sdk_checkout.clone(),
        ))
        .unwrap();
        let refused = |sdk: serde_json::Value, expected: &str| {
            let error = check(sdk).unwrap_err();
            assert!(error.contains(expected), "{expected}: {error}");
        };
        // A Cargo config `paths` override or `[patch]` gives it a path...
        refused(
            package(
                "bloom-petal-sdk",
                "0.1.0",
                None,
                tmp.path().join("evil/Cargo.toml"),
            ),
            "bloom-petal-sdk 0.1.0 is read from",
        );
        // ...even inside Cargo home, and with another version.
        refused(
            package("bloom-petal-sdk", "0.1.0", None, sdk_checkout.clone()),
            "which the tested lockfile does not name",
        );
        refused(
            package(
                "bloom-petal-sdk",
                "0.1.1",
                None,
                tmp.path().join("evil/Cargo.toml"),
            ),
            "bloom-petal-sdk 0.1.1 is read from",
        );
        // A source replacement keeps the source but reads other files, even
        // from inside Cargo home's own folders.
        for elsewhere in [
            tmp.path().join("vendor/bloom-petal-sdk/Cargo.toml"),
            home.join("vendor/bloom-petal-sdk/Cargo.toml"),
            home.join("registry/src/vendored/bloom-petal-sdk-0.1.0/Cargo.toml"),
            home.join("git/checkouts/petal-0123456789abcdef/2beed2f/crates/petal-sdk/Cargo.toml"),
            home.join("git/checkouts/vendored/2225574/crates/petal-sdk/Cargo.toml"),
        ] {
            refused(
                package("bloom-petal-sdk", "0.1.0", Some(&git), elsewhere.clone()),
                &format!(
                    "is read from {}, not Cargo's copy of {git}",
                    elsewhere.display()
                ),
            );
        }
        // A registry package outside Cargo's `<index>/<name>-<version>` layout.
        let vendored_hex = serde_json::json!({"packages": [
            package("hex", "0.4.3", Some(registry), home.join("registry/src/vendored/hex/Cargo.toml")),
        ]});
        assert!(
            require_locked_sources(
                &tested,
                &vendored_hex,
                &home,
                &generated,
                &BTreeSet::new(),
                &BTreeSet::new(),
            )
            .unwrap_err()
            .contains("hex 0.4.3 is read from")
        );
        // A source the lockfile does not name.
        refused(
            package(
                "bloom-petal-sdk",
                "0.1.0",
                Some(&format!("git+{SDK_GIT}?rev={REV_B}#{REV_B}")),
                sdk_checkout.clone(),
            ),
            "is not in the tested lockfile",
        );
        // A locked path package read from outside the checkout.
        let metadata = serde_json::json!({"packages": [
            package("demo-route", "0.1.0", None, tmp.path().join("elsewhere/route/Cargo.toml")),
        ]});
        let packages = BTreeSet::from([checkout.join("route")]);
        let only = |metadata: serde_json::Value| {
            require_locked_sources(
                &tested,
                &metadata,
                &home,
                &generated,
                &BTreeSet::from(["petal-route-status"]),
                &packages,
            )
        };
        assert!(
            only(metadata)
                .unwrap_err()
                .contains("demo-route 0.1.0 is read from")
        );
        // So is a copy of it elsewhere in the checkout, and a wrapper read
        // from anywhere but where it was generated.
        for (name, manifest) in [
            (
                "demo-route",
                checkout.join("fixtures/route-copy/Cargo.toml"),
            ),
            ("petal-route-status", checkout.join("elsewhere/Cargo.toml")),
        ] {
            let metadata =
                serde_json::json!({"packages": [package(name, "0.1.0", None, manifest)]});
            assert!(
                only(metadata)
                    .unwrap_err()
                    .contains(&format!("{name} 0.1.0 is read from")),
                "{name}"
            );
        }
    }

    #[test]
    fn release_builds_refuse_toolchains_named_by_path() {
        let tmp = tempfile::tempdir().unwrap();
        let cwd = tmp.path().join("work/petal");
        fs::create_dir_all(&cwd).unwrap();
        // A channel, in either form, names a toolchain rustup installs.
        fs::write(
            cwd.join("rust-toolchain.toml"),
            "[toolchain]\nchannel = \"1.98.0\"\n",
        )
        .unwrap();
        fs::write(tmp.path().join("work/rust-toolchain"), "stable\n").unwrap();
        assert!(custom_toolchain_files(&cwd).is_empty());
        fs::write(
            cwd.join("rust-toolchain.toml"),
            "[toolchain]\npath = \"/opt/rustc\"\n",
        )
        .unwrap();
        fs::write(tmp.path().join("rust-toolchain"), "/opt/rustc\n").unwrap();
        assert_eq!(
            custom_toolchain_files(&cwd),
            [
                cwd.join("rust-toolchain.toml"),
                tmp.path().join("rust-toolchain")
            ]
        );
    }

    #[test]
    fn percent_decoding_never_splits_a_character() {
        assert_eq!(percent_decode("feat%2Fx"), "feat/x");
        assert_eq!(percent_decode("x%a€"), "x%a€");
        assert_eq!(percent_decode("%2"), "%2");
        assert_eq!(percent_decode("%2541"), "%41");
    }

    /// Cargo applies `..` to a path dependency as written, so the generated
    /// workspace must reach a symlinked route crate the same way, or its
    /// `..` dependencies resolve elsewhere than in the route's tests.
    #[cfg(unix)]
    #[test]
    fn route_sources_follow_paths_as_cargo_writes_them() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().canonicalize().unwrap().join("petal");
        let outside = tmp.path().canonicalize().unwrap().join("outside");
        let write = |path: &Path, body: &str| {
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(path, body).unwrap();
        };
        let package = |name: &str| format!("[package]\nname = \"{name}\"\nversion = \"0.1.0\"\n");
        write(
            &root.join("crates/route/Cargo.toml"),
            &format!(
                "{}[workspace]\n[dependencies]\npetal = {{ package = \"bloom-petal-sdk\", version = \"=0.1.0\" }}\nshared = {{ path = \"../shared\" }}\nvendored = {{ path = \"../vendor/vendored\" }}\n[dev-dependencies]\ntools = {{ path = \"../../../tools\" }}\n",
                package("demo-route")
            ),
        );
        std::os::unix::fs::symlink(root.join("crates/route"), root.join("route")).unwrap();
        write(&root.join("shared/Cargo.toml"), &package("shared"));
        write(&root.join("crates/shared/Cargo.toml"), &package("shared"));
        write(&outside.join("vendored/Cargo.toml"), &package("vendored"));
        std::os::unix::fs::symlink(&outside, root.join("vendor")).unwrap();
        let mut config = package_config();
        config.sdk = SdkDependency::default();
        let sources = route_sources(&root, &config).unwrap();
        // `route/../shared` is `shared`, not `crates/shared`; the vendored
        // dependency stays inside the Petal through its link; and a
        // dev-dependency outside it, which components never build, does not
        // move the anchor.
        assert_eq!(sources.anchor, root);
        assert_eq!(
            sources.packages,
            BTreeSet::from([
                root.join("route"),
                root.join("shared"),
                root.join("vendor/vendored"),
            ])
        );
        let workspace = root.join("target/petal-routes/workspace");
        let mut status = route("status.json");
        status.source = root.join("route/files/status.json.rs");
        generate_workspace(&root, &workspace, &[status.clone()], &config).unwrap();
        let member = fs::read_to_string(
            workspace
                .join("members")
                .join(&status.package)
                .join("Cargo.toml"),
        )
        .unwrap();
        assert!(member.contains("path = \"../../source/route\""), "{member}");
    }

    #[test]
    fn release_builds_see_the_checkouts_cargo_config_but_not_cargo_homes() {
        let tmp = tempfile::tempdir().unwrap();
        let cwd = tmp.path().join("work/petal");
        fs::create_dir_all(cwd.join(".cargo")).unwrap();
        fs::create_dir_all(tmp.path().join(".cargo")).unwrap();
        assert!(checkout_cargo_config(&cwd, None).is_empty());
        fs::write(cwd.join(".cargo/config.toml"), "").unwrap();
        fs::write(tmp.path().join(".cargo/config"), "").unwrap();
        assert_eq!(
            checkout_cargo_config(&cwd, None),
            [
                cwd.join(".cargo/config.toml"),
                tmp.path().join(".cargo/config")
            ]
        );
        // Cargo home's own config is the machine's, not the checkout's.
        assert_eq!(
            checkout_cargo_config(&cwd, Some(&tmp.path().join(".cargo"))),
            [cwd.join(".cargo/config.toml")]
        );
    }

    /// Cargo passes over a workspace that excludes a crate without also listing
    /// it in `members`, and `package.workspace` names the workspace outright.
    #[test]
    fn route_workspaces_are_found_by_cargo_rules() {
        let tmp = tempfile::tempdir().unwrap();
        let write = |path: &str, body: &str| {
            let path = tmp.path().join(path);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(path, body).unwrap();
        };
        let package = "[package]\nname = \"demo-route\"\nversion = \"0.1.0\"\nedition = \"2024\"\n";
        let found = |route: &str| {
            let dir = tmp.path().join(route);
            workspace_dir(&dir, &read_toml(&dir.join("Cargo.toml")).unwrap()).unwrap()
        };
        // The nearer workspace excludes the crate; the outer one lists it.
        write("a/Cargo.toml", "[workspace]\nmembers = [\"b/c/route\"]\n");
        write("a/b/Cargo.toml", "[workspace]\nexclude = [\"c\"]\n");
        write("a/b/c/route/Cargo.toml", package);
        assert_eq!(found("a/b/c/route"), tmp.path().join("a"));
        // An explicit member belongs even under an excluded path.
        write(
            "m/Cargo.toml",
            "[workspace]\nmembers = [\"petals/demo/route\"]\nexclude = [\"petals\"]\n",
        );
        write("m/petals/demo/route/Cargo.toml", package);
        assert_eq!(found("m/petals/demo/route"), tmp.path().join("m"));
        // Excluded with no workspace further up: the crate is its own.
        write("x/Cargo.toml", "[workspace]\nexclude = [\"route\"]\n");
        write("x/route/Cargo.toml", package);
        assert_eq!(found("x/route"), tmp.path().join("x/route"));
        // `package.workspace` names the workspace directly.
        write("p/ws/Cargo.toml", "[workspace]\nmembers = [\"../route\"]\n");
        write(
            "p/route/Cargo.toml",
            &format!("{package}workspace = \"../ws\"\n"),
        );
        assert_eq!(found("p/route"), tmp.path().join("p/ws"));
        // So does an ancestor package's, for the crates below it.
        write(
            "q/ws/Cargo.toml",
            "[workspace]\nmembers = [\"../pkg\", \"../pkg/route\"]\n",
        );
        write(
            "q/pkg/Cargo.toml",
            &format!("{package}workspace = \"../ws\"\n"),
        );
        write("q/pkg/route/Cargo.toml", package);
        assert_eq!(found("q/pkg/route"), tmp.path().join("q/ws"));
    }

    /// In a monorepo, the route crate's workspace and lockfile can sit above
    /// the Petal root; Cargo finds them there, and so must the pin check.
    #[test]
    fn sdk_pin_finds_the_route_workspace_above_the_petal_root() {
        let tmp = tempfile::tempdir().unwrap();
        let mono = tmp.path().join("mono");
        let root = mono.join("petals/demo");
        fs::create_dir_all(root.join("route")).unwrap();
        let route = |dependency: &str| {
            fs::write(
                root.join("route/Cargo.toml"),
                format!("[package]\nname = \"demo-route\"\nversion = \"0.1.0\"\nedition = \"2024\"\n\n[dependencies]\n{dependency}"),
            )
            .unwrap();
        };
        route("petal.workspace = true\n");
        fs::write(
            mono.join("Cargo.toml"),
            format!(
                "[workspace]\nmembers = [\"petals/demo/route\"]\n\n[workspace.dependencies]\n{}",
                git_dep("petal", "bloom-petal-sdk", REV_A)
            ),
        )
        .unwrap();
        let mut config = package_config();
        config.sdk = SdkDependency::default();
        check_sdk_pin(&root, &config).unwrap();
        fs::write(mono.join("Cargo.lock"), git_lock(REV_B)).unwrap();
        let error = check_sdk_pin(&root, &config).unwrap_err();
        assert!(
            error.contains("Cargo.lock resolves bloom-petal-sdk"),
            "{error}"
        );

        // A workspace that excludes the Petal leaves the route crate on its
        // own, so that workspace's lockfile is not the route's.
        route(&git_dep("petal", "bloom-petal-sdk", REV_A));
        fs::write(
            mono.join("Cargo.toml"),
            "[workspace]\nexclude = [\"petals\"]\n",
        )
        .unwrap();
        check_sdk_pin(&root, &config).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn rustflags_that_are_not_utf8_are_refused() {
        use std::os::unix::ffi::OsStringExt;
        assert_eq!(utf8_rustflags("RUSTFLAGS", None), Ok(None));
        assert_eq!(
            utf8_rustflags("RUSTFLAGS", Some("--cfg probe".into())),
            Ok(Some("--cfg probe".into()))
        );
        assert_eq!(
            utf8_rustflags(
                "CARGO_BUILD_RUSTFLAGS",
                Some(std::ffi::OsString::from_vec(vec![0xff]))
            ),
            Err("CARGO_BUILD_RUSTFLAGS must be UTF-8".into())
        );
    }

    /// Cargo config and `CARGO_PROFILE_RELEASE_*` variables override the
    /// generated manifest's profile, so the build repeats every key on the
    /// command line. scripts/test-component-reproducibility.sh checks that
    /// such overrides leave the components unchanged.
    #[test]
    fn the_component_profile_is_also_set_on_the_command_line() {
        let config = component_profile_config();
        assert_eq!(config.len(), COMPONENT_PROFILE.len());
        for setting in [
            "profile.release.codegen-units=1",
            "profile.release.strip=\"symbols\"",
            "profile.release.opt-level=3",
        ] {
            assert!(config.iter().any(|value| value == setting), "{config:?}");
        }
    }

    #[test]
    fn canonicalizes_indexes() {
        assert_eq!(canonical_route_path("$index"), "");
        assert_eq!(canonical_route_path("a/$index"), "a");
    }
    #[test]
    fn extracts_typed_params() {
        assert_eq!(
            route_params("trade/[wallet]/[id]/file.json"),
            vec![("wallet", 1), ("id", 2)]
        );
    }

    fn route(path: &str) -> Route {
        Route {
            path: path.to_owned(),
            canonical: canonical_route_path(path),
            source: PathBuf::from(format!("{path}.rs")),
            package: package_name(path),
        }
    }

    #[test]
    fn route_sources_follow_every_declared_path_dependency() {
        let tmp = tempfile::tempdir().unwrap();
        let outer = tmp.path().canonicalize().unwrap();
        let root = outer.join("a/petal");
        let write = |path: &Path, body: &str| {
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(path, body).unwrap();
        };
        let package = |name: &str| format!("[package]\nname = \"{name}\"\nversion = \"0.1.0\"\n");
        // The route crate inherits one path dependency from its workspace and
        // builds with another, which has a path dependency of its own.
        write(
            &root.join("Cargo.toml"),
            "[workspace]\nmembers = [\"route\"]\n[workspace.dependencies]\ninherited = { path = \"../inherited\" }\n",
        );
        write(
            &root.join("route/Cargo.toml"),
            &format!(
                "{}[dependencies]\ninherited.workspace = true\n[build-dependencies]\ngenerator = {{ path = \"../../../generator\" }}\n",
                package("demo-route")
            ),
        );
        write(&outer.join("a/inherited/Cargo.toml"), &package("inherited"));
        write(
            &outer.join("generator/Cargo.toml"),
            &format!(
                "{}[dependencies]\ndeep = {{ path = \"../deep/x\" }}\n",
                package("generator")
            ),
        );
        write(&outer.join("deep/x/Cargo.toml"), &package("deep"));
        let mut config = package_config();
        config.sdk = SdkDependency::default();
        let sources = route_sources(&root, &config).unwrap();
        assert_eq!(sources.anchor, outer);
        assert_eq!(
            sources.packages,
            BTreeSet::from([
                root.join("route"),
                outer.join("a/inherited"),
                outer.join("generator"),
                outer.join("deep/x"),
            ])
        );
    }

    /// Cargo hashes a path package's absolute location into its symbol
    /// names unless the package is inside the workspace, so the generated
    /// workspace reaches the route crate and a path SDK through its link.
    #[cfg(unix)]
    #[test]
    fn generated_workspaces_reach_the_checkout_through_their_source_link() {
        let tmp = tempfile::tempdir().unwrap();
        let outer = tmp.path().canonicalize().unwrap();
        let root = outer.join("petal");
        fs::create_dir_all(root.join("route/files")).unwrap();
        fs::create_dir_all(root.join("sdk")).unwrap();
        let manifest = |extra: &str| {
            fs::write(
                root.join("route/Cargo.toml"),
                format!("[package]\nname = \"demo-route\"\nversion = \"0.1.0\"\nedition = \"2024\"\n[workspace]\n[dependencies]\npetal = {{ package = \"bloom-petal-sdk\", path = \"../sdk\" }}\n{extra}"),
            )
            .unwrap();
        };
        manifest("");
        let mut config = package_config();
        config.sdk = SdkDependency::default();
        assert_eq!(route_sources(&root, &config).unwrap().anchor, root);

        let workspace = root.join("target/petal-routes/workspace");
        let mut status = route("status.json");
        status.source = root.join("route/files/status.json.rs");
        generate_workspace(&root, &workspace, &[status.clone()], &config).unwrap();
        assert_eq!(
            fs::read_link(workspace.join("source")).unwrap(),
            Path::new("../../..")
        );
        let generated = fs::read_to_string(workspace.join("Cargo.toml")).unwrap();
        assert!(generated.contains("exclude = [\"source\"]"), "{generated}");
        let member = fs::read_to_string(
            workspace
                .join("members")
                .join(&status.package)
                .join("Cargo.toml"),
        )
        .unwrap();
        assert!(member.contains("path = \"../../source/route\""), "{member}");
        assert!(member.contains("path = \"../../source/sdk\""), "{member}");
        // Regenerating keeps the link.
        generate_workspace(&root, &workspace, &[status.clone()], &config).unwrap();
        assert_eq!(
            fs::read_link(workspace.join("source")).unwrap(),
            Path::new("../../..")
        );

        // A path dependency outside the Petal moves the link up to hold it.
        fs::create_dir_all(outer.join("shared")).unwrap();
        fs::write(
            outer.join("shared/Cargo.toml"),
            "[package]\nname = \"shared\"\nversion = \"0.1.0\"\n",
        )
        .unwrap();
        manifest("shared = { path = \"../../shared\" }\n");
        assert_eq!(route_sources(&root, &config).unwrap().anchor, outer);
        generate_workspace(&root, &workspace, &[status.clone()], &config).unwrap();
        assert_eq!(
            fs::read_link(workspace.join("source")).unwrap(),
            Path::new("../../../..")
        );
        let member = fs::read_to_string(
            workspace
                .join("members")
                .join(&status.package)
                .join("Cargo.toml"),
        )
        .unwrap();
        assert!(
            member.contains("path = \"../../source/petal/route\""),
            "{member}"
        );
    }

    #[test]
    fn a_wallet_capture_needs_an_adjacent_account_capture() {
        let migrated = [
            route("swaps/[wallet]/$index"),
            route("swaps/[wallet]/[index]/$index"),
            route("swaps/[wallet]/[index]/new"),
            route("swaps/[wallet]/[index]/[id]/status.json"),
            route("status.json"),
        ];
        assert!(route_shape_violations(&migrated).is_empty());

        // The shape every unmigrated Petal is in: Bloom denies all of these.
        let unmigrated = [route("swaps/[wallet]/$index"), route("swaps/[wallet]/new")];
        let found = route_shape_violations(&unmigrated);
        // The listing counts too once nothing declares [index] beneath it,
        // which is what makes an unmigrated tree fail as a whole.
        assert_eq!(found.len(), 2, "{found:?}");
        assert!(found.iter().any(|v| v.contains("swaps/[wallet]/$index")));
        assert!(found.iter().any(|v| v.contains("swaps/[wallet]/new")));
    }

    #[test]
    fn a_suffixed_capture_is_refused_even_though_bloom_binds_it() {
        // Bloom parses `[wallet].json` as the `wallet` param, so these routes
        // look wallet-scoped, but its adjacency check compares exact segments.
        for path in ["balance/[wallet].json", "balance/[wallet]/[index].json"] {
            let found = route_shape_violations(&[route(path)]);
            assert_eq!(found.len(), 1, "{path}: {found:?}");
            assert!(found[0].contains("exact segments"), "{found:?}");
        }
        // A capture whose name merely starts with `wallet` is not a wallet.
        assert!(route_shape_violations(&[route("keys/[wallet_id].json")]).is_empty());
    }

    #[test]
    fn suffixed_captures_are_reported_once_per_route() {
        // A suffixed account capture with no wallet before it is one route
        // with one problem.
        let found = route_shape_violations(&[route("pages/[index].json")]);
        assert_eq!(found.len(), 1, "{found:?}");
        // The morpho #5 shape: the listing points at a suffixed account route,
        // which Bloom refuses, so the listing has no account route to serve.
        let found = route_shape_violations(&[
            route("positions/[wallet]/$index"),
            route("positions/[wallet]/[index].json"),
        ]);
        assert_eq!(found.len(), 2, "{found:?}");
        assert!(
            found
                .iter()
                .any(|v| v.contains("positions/[wallet]/$index"))
        );
        assert!(found.iter().any(|v| v.contains("exact segments")));
    }

    #[test]
    fn an_account_capture_needs_a_preceding_wallet() {
        let orphan = [route("sessions/[index]/new")];
        let found = route_shape_violations(&orphan);
        assert_eq!(found.len(), 1, "{found:?}");
        assert!(found[0].contains("without a preceding [wallet]"));
    }
    #[test]
    fn package_names_are_stable_and_collision_safe() {
        assert_eq!(package_name("a[b]"), package_name("a[b]"));
        assert_ne!(package_name("a[b]"), package_name("a-b"));
    }

    #[test]
    fn shared_route_code_cannot_dispatch_on_route_identity() {
        let fixture = tempfile::tempdir().unwrap();
        let source = fixture.path().join("route/src");
        fs::create_dir_all(&source).unwrap();
        fs::write(source.join("lib.rs"), "pub fn shared() {}\n").unwrap();
        check_shared_route_architecture(fixture.path(), &package_config()).unwrap();

        fs::write(
            source.join("lib.rs"),
            "pub fn shared(ctx: &petal::Ctx) { let _ = petal::current_route_path(ctx); }\n",
        )
        .unwrap();
        let error = check_shared_route_architecture(fixture.path(), &package_config()).unwrap_err();
        assert!(error.contains("forbidden route-identity accessor current_route_path"));
        assert!(error.contains("route/src/lib.rs"));

        fs::write(
            source.join("lib.rs"),
            "/// Route files own current_route_path dispatch.\npub fn current_route_path_label() -> &'static str { \"shared\" }\n",
        )
        .unwrap();
        check_shared_route_architecture(fixture.path(), &package_config()).unwrap();
    }
}
