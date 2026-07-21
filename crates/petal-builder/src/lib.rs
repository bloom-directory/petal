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
    validate_sdk(&config.sdk)?;
    Ok(config)
}

pub fn build(root: impl AsRef<Path>, config: &BuildConfig) -> Result<BuildReport, String> {
    build_inner(root.as_ref(), config, false)
}

pub fn check_caps(root: impl AsRef<Path>, config: &BuildConfig) -> Result<BuildReport, String> {
    build_inner(root.as_ref(), config, true)
}

/// Create the strict, deterministic `.petal.tar.gz` consumed by Bloom.
///
/// Callers should build and check their routes first. The CLI's `package`
/// command does that check automatically before calling this function.
pub fn package(
    root: impl AsRef<Path>,
    config: &BuildConfig,
    out: impl AsRef<Path>,
) -> Result<PackageReport, String> {
    let root = root
        .as_ref()
        .canonicalize()
        .map_err(|error| format!("resolve {}: {error}", root.as_ref().display()))?;
    let out = absolute_from(&root, out.as_ref());
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
    for required in ["README.md", "AGENTS.md"] {
        let path = root.join(required);
        if !path.is_file() {
            return Err(format!("Petal package missing required file {required}"));
        }
    }

    let routes = discover_wasm_paths(&root.join(&config.output))?;
    if routes.is_empty() {
        return Err(format!(
            "Petal package contains no route components under {}",
            config.output.display()
        ));
    }
    let mut files = Vec::new();
    collect_package_files(&root, &root, &out, &mut files)?;
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

fn absolute_from(root: &Path, path: &Path) -> PathBuf {
    if path.is_absolute() {
        path.to_owned()
    } else {
        root.join(path)
    }
}

fn collect_package_files(
    root: &Path,
    dir: &Path,
    output: &Path,
    files: &mut Vec<PackageFile>,
) -> Result<(), String> {
    let mut entries = fs::read_dir(dir)
        .map_err(display_err("read", dir))?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| format!("read {}: {error}", dir.display()))?;
    entries.sort_by_key(|entry| entry.file_name());
    for entry in entries {
        let path = entry.path();
        if path == output {
            continue;
        }
        let ty = entry
            .file_type()
            .map_err(|error| format!("inspect {}: {error}", path.display()))?;
        if ty.is_dir() {
            if should_skip_package_dir(root, &path) {
                continue;
            }
            collect_package_files(root, &path, output, files)?;
        } else if ty.is_file() {
            if path
                .file_name()
                .and_then(OsStr::to_str)
                .is_some_and(|name| name.ends_with(".petal.tar") || name.ends_with(".petal.tar.gz"))
            {
                continue;
            }
            let relative = path
                .strip_prefix(root)
                .map_err(|_| format!("package file escaped root: {}", path.display()))?;
            let relative = package_path(relative)?;
            validate_ustar_path(&relative)?;
            files.push(PackageFile {
                path: relative,
                bytes: fs::read(&path).map_err(display_err("read", &path))?,
            });
        } else {
            return Err(format!(
                "Petal package contains non-regular file {}",
                path.display()
            ));
        }
    }
    Ok(())
}

fn should_skip_package_dir(root: &Path, path: &Path) -> bool {
    let Ok(relative) = path.strip_prefix(root) else {
        return false;
    };
    matches!(
        relative
            .components()
            .next()
            .and_then(|component| component.as_os_str().to_str()),
        Some(".git" | ".jj" | "artifacts" | "target")
    ) || path.file_name().and_then(OsStr::to_str) == Some("target")
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
    format!("{:x}", Sha256::digest(bytes))
}

fn build_inner(root: &Path, config: &BuildConfig, check_only: bool) -> Result<BuildReport, String> {
    let root = root
        .canonicalize()
        .map_err(|error| format!("resolve {}: {error}", root.display()))?;
    let routes = discover_routes(&root.join(&config.routes))?;
    if routes.is_empty() {
        return Err("no route controllers found".into());
    }
    let out_dir = root.join(&config.output);

    if check_only {
        let failures = routes
            .iter()
            .filter_map(|route| {
                check_route_caps(route, &out_dir.join(format!("{}.wasm", route.path))).err()
            })
            .collect::<Vec<_>>();
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
    generate_lockfile(&workspace)?;
    let artifacts = build_workspace(&workspace, &routes, config.jobs)?;

    let staging = build_root.join("staging").join(&config.name);
    if staging.exists() {
        fs::remove_dir_all(&staging).map_err(display_err("remove", &staging))?;
    }
    fs::create_dir_all(&staging).map_err(display_err("create", &staging))?;
    let mut failures = Vec::new();
    for route in &routes {
        let result = (|| {
            let core = artifacts
                .get(&route.package)
                .ok_or_else(|| format!("no artifact for {}", route.package))?;
            let output = staging.join(format!("{}.wasm", route.path));
            fs::create_dir_all(output.parent().unwrap())
                .map_err(display_err("create", output.parent().unwrap()))?;
            command(
                Command::new("wasm-tools")
                    .args(["component", "new"])
                    .arg(core)
                    .arg("-o")
                    .arg(&output),
            )?;
            command(Command::new("wasm-tools").arg("validate").arg(&output))?;
            check_route_caps(route, &output)
        })();
        if let Err(err) = result {
            failures.push(err);
        }
    }
    if !failures.is_empty() {
        return Err(failures.join("\n"));
    }
    let staged = discover_wasm_paths(&staging)?;
    let expected = routes
        .iter()
        .map(|r| format!("{}.wasm", r.path))
        .collect::<BTreeSet<_>>();
    if staged != expected {
        return Err("staging route set differs from discovered controllers".into());
    }

    fs::create_dir_all(out_dir.parent().unwrap())
        .map_err(display_err("create", out_dir.parent().unwrap()))?;
    replace_dir(&staging, &out_dir)?;
    println!(
        "wrote {} route components under {}",
        routes.len(),
        out_dir.display()
    );
    let report = report(config, &routes, &out_dir)?;
    let report_path = build_root.join("build-report.json");
    write_if_changed(
        &report_path,
        &serde_json::to_string_pretty(&report).map_err(|error| error.to_string())?,
    )?;
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
    write_if_changed(
        &workspace.join("Cargo.toml"),
        &format!(
            "[workspace]\nresolver = \"2\"\nmembers = [\n{members}\n]\n\n[profile.release]\nopt-level = 3\ndebug = false\nstrip = \"none\"\ndebug-assertions = false\noverflow-checks = false\nlto = false\npanic = \"unwind\"\nincremental = false\ncodegen-units = 16\nrpath = false\n"
        ),
    )?;
    let wanted = routes
        .iter()
        .map(|r| r.package.as_str())
        .collect::<BTreeSet<_>>();
    let members_dir = workspace.join("members");
    fs::create_dir_all(&members_dir).map_err(display_err("create", &members_dir))?;
    for entry in fs::read_dir(&members_dir).map_err(display_err("read", &members_dir))? {
        let path = entry.map_err(|e| e.to_string())?.path();
        if path.is_dir() && !wanted.contains(path.file_name().and_then(OsStr::to_str).unwrap_or(""))
        {
            fs::remove_dir_all(&path).map_err(display_err("prune", &path))?;
        }
    }
    for route in routes {
        let dir = members_dir.join(&route.package);
        let sdk = dependency_toml(&config.sdk, root)?;
        let route_path = root.join(&config.route_crate.path);
        let route_path = path_from_member(&dir, &route_path)?;
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
            "#![allow(clippy::too_many_arguments)]\n#![allow(dead_code, unused_imports, clippy::upper_case_acronyms)]\n\npub struct __PetalRouteIdentity;\nimpl petal::RouteIdentity for __PetalRouteIdentity {{\n    const PATH: &'static str = {:?};\n    const CANONICAL_PATH: &'static str = {:?};\n    const PARAMS: &'static [(&'static str, usize)] = &[\n{}\n    ];\n}}\n\npub use {}::*;\n\nmod selected_route {{\n    include!({:?});\n}}\n\nuse selected_route::Route;\npetal::bindings::export!(Route);\n",
            route.path, route.canonical, params, config.route_crate.alias, source_path,
        );
        write_if_changed(&dir.join("Cargo.toml"), &manifest)?;
        write_if_changed(&dir.join("src/lib.rs"), &source)?;
    }
    Ok(())
}

fn generate_lockfile(workspace: &Path) -> Result<(), String> {
    command(
        Command::new("cargo")
            .arg("generate-lockfile")
            .arg("--manifest-path")
            .arg(workspace.join("Cargo.toml")),
    )
}

fn build_workspace(
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

fn discover_wasm_paths(root: &Path) -> Result<BTreeSet<String>, String> {
    fn walk(root: &Path, dir: &Path, out: &mut BTreeSet<String>) -> Result<(), String> {
        for entry in fs::read_dir(dir).map_err(display_err("read", dir))? {
            let path = entry.map_err(|e| e.to_string())?.path();
            if path.is_dir() {
                walk(root, &path, out)?;
            } else if path.extension() == Some(OsStr::new("wasm")) {
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
    if let Some(path) = &sdk.path {
        validate_relative(path, "sdk.path")?;
    }
    Ok(())
}

fn dependency_toml(sdk: &SdkDependency, root: &Path) -> Result<String, String> {
    let mut table = toml::map::Map::new();
    table.insert("package".into(), toml::Value::String(sdk.package.clone()));
    if let Some(version) = &sdk.version {
        table.insert("version".into(), toml::Value::String(version.clone()));
    }
    if let Some(path) = &sdk.path {
        table.insert(
            "path".into(),
            toml::Value::String(root.join(path).to_string_lossy().into_owned()),
        );
    }
    if let Some(git) = &sdk.git {
        table.insert("git".into(), toml::Value::String(git.clone()));
    }
    if let Some(rev) = &sdk.rev {
        table.insert("rev".into(), toml::Value::String(rev.clone()));
    }
    Ok(toml::Value::Table(table).to_string())
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

fn replace_dir(staging: &Path, destination: &Path) -> Result<(), String> {
    let backup = destination.with_extension("petal-builder-backup");
    if backup.exists() {
        fs::remove_dir_all(&backup).map_err(display_err("remove", &backup))?;
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
    if backup.exists() {
        fs::remove_dir_all(&backup).map_err(display_err("remove", &backup))?;
    }
    Ok(())
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
    #[test]
    fn package_names_are_stable_and_collision_safe() {
        assert_eq!(package_name("a[b]"), package_name("a[b]"));
        assert_ne!(package_name("a[b]"), package_name("a-b"));
    }
}
