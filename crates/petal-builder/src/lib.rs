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
    check_sdk_pin(&root, config)?;
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
    let runtime = PathBuf::from("petal").join(&config.name);
    if config.output != runtime {
        return Err(format!(
            "installable package output must be {}, got {}",
            runtime.display(),
            config.output.display()
        ));
    }
    if out.starts_with(root.join("petal")) {
        return Err("archive output must be outside the runtime tree".into());
    }
    // These are the host-consumed inputs. Keep all runtime files and the full
    // security manifest; development reports and source bookkeeping are not
    // installation identity. Machine composition still assigns its own hash.
    for path in ["petal.toml", "README.md", "AGENTS.md", "petal"] {
        collect_package_files(&root, &root.join(path), &mut files)?;
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

fn absolute_from(root: &Path, path: &Path) -> PathBuf {
    if path.is_absolute() {
        path.to_owned()
    } else {
        root.join(path)
    }
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
    reuse_route_lockfile(&root, &workspace, config, &routes)?;
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
    fs::create_dir_all(staging.parent().unwrap()).map_err(display_err("create", &build_root))?;
    fs::create_dir(&staging).map_err(display_err("create", &staging))?;
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
            "[workspace]\nresolver = \"2\"\nmembers = [\n{members}\n]\n\n[profile.release]\nopt-level = 3\ndebug = false\nstrip = \"symbols\"\ndebug-assertions = false\noverflow-checks = false\nlto = false\npanic = \"unwind\"\nincremental = false\ncodegen-units = 16\nrpath = false\n"
        ),
    )?;
    let members_dir = workspace.join("members");
    fs::create_dir_all(&members_dir).map_err(display_err("create", &members_dir))?;
    for route in routes {
        let dir = members_dir.join(&route.package);
        let sdk = route_sdk_dependency(root, config)?.to_string();
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
) -> Result<(), String> {
    let route_dir = root.join(&config.route_crate.path);
    let manifest = read_toml(&route_dir.join("Cargo.toml"))?;
    let original = workspace_dir(root, &route_dir, &manifest)?.join("Cargo.lock");
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
    validate_locked_graph(&old, &new, &wrappers)
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
                    package.get("source").and_then(toml::Value::as_str) == Some(source)
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
    let flags = component_rustflags(root);
    if std::env::var_os("CARGO_ENCODED_RUSTFLAGS").is_some()
        || std::env::var_os("RUSTFLAGS").is_some()
    {
        cmd.env("CARGO_ENCODED_RUSTFLAGS", flags);
    } else {
        // Cargo appends this array to configured build flags. Do not replace
        // a developer's target-specific flags with an environment override.
        let flags = flags
            .to_str()
            .ok_or("component rustflags must be UTF-8")?
            .split('\x1f')
            .collect::<Vec<_>>();
        cmd.arg("--config").arg(format!(
            "build.rustflags={}",
            serde_json::to_string(&flags).map_err(|error| error.to_string())?
        ));
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

fn component_rustflags(root: &Path) -> std::ffi::OsString {
    let mut flags = std::env::var_os("CARGO_ENCODED_RUSTFLAGS").unwrap_or_else(|| {
        std::env::var("RUSTFLAGS")
            .unwrap_or_default()
            .split_whitespace()
            .collect::<Vec<_>>()
            .join("\x1f")
            .into()
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
    if let Some(cargo_home) = std::env::var_os("CARGO_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".cargo")))
    {
        remap(&cargo_home, "/cargo");
    }
    flags
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
    // An exact pin, so every build and every test resolves the same SDK.
    if sdk.git.is_some() && !sdk.rev.as_deref().is_some_and(is_full_commit) {
        return Err("sdk.git requires sdk.rev, a full 40-character commit".into());
    }
    if let Some(version) = &sdk.version
        && !is_exact_version(version)
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
    let workspace = workspace_dir(root, &crate_dir, &manifest)?;
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
        if inherited {
            for field in ["features", "default-features"] {
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
            let path = resolve_sdk_path(base, Path::new(path))?;
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
        if bytes[index] == b'%'
            && index + 2 < bytes.len()
            && let Ok(byte) = u8::from_str_radix(&text[index + 1..index + 3], 16)
        {
            out.push(byte);
            index += 3;
            continue;
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

/// `[patch.<source>]` and `[replace]` entries that override `package`.
fn overrides(manifest: &toml::Table, package: &str) -> Vec<String> {
    let mut found = Vec::new();
    if let Some(patches) = manifest.get("patch").and_then(toml::Value::as_table) {
        for (source, table) in patches {
            for (key, value) in table.as_table().into_iter().flatten() {
                if dependency_package(key, value) == package {
                    found.push(format!("[patch.{source:?}] {key}"));
                }
            }
        }
    }
    if let Some(replace) = manifest.get("replace").and_then(toml::Value::as_table) {
        for spec in replace.keys() {
            let name = spec.split([':', '@']).next().unwrap_or(spec);
            if name == package {
                found.push(format!("[replace] {spec:?}"));
            }
        }
    }
    found
}

fn read_toml(path: &Path) -> Result<toml::Table, String> {
    let body = fs::read_to_string(path).map_err(display_err("read", path))?;
    toml::from_str(&body).map_err(|error| format!("parse {}: {error}", path.display()))
}

/// The directory of the workspace the route crate belongs to: the route crate
/// itself when it declares `[workspace]` or has no workspace above it inside
/// the package root.
fn workspace_dir(root: &Path, crate_dir: &Path, manifest: &toml::Table) -> Result<PathBuf, String> {
    if manifest.contains_key("workspace") {
        return Ok(crate_dir.to_owned());
    }
    for dir in crate_dir.ancestors().skip(1) {
        if !dir.starts_with(root) {
            break;
        }
        let path = dir.join("Cargo.toml");
        if path.is_file() && read_toml(&path)?.contains_key("workspace") {
            return Ok(dir.to_owned());
        }
    }
    Ok(crate_dir.to_owned())
}

/// Whether a `.cargo/config.toml` that Cargo reads for the route crate patches
/// `package`. This is how the petal repository's consumer CI builds a Petal
/// against its own checkout.
fn patched_by_cargo_config(root: &Path, crate_dir: &Path, package: &str) -> Result<bool, String> {
    for dir in crate_dir.ancestors() {
        for name in ["config.toml", "config"] {
            let path = dir.join(".cargo").join(name);
            if path.is_file() && !overrides(&read_toml(&path)?, package).is_empty() {
                return Ok(true);
            }
        }
        if dir == root {
            break;
        }
    }
    Ok(false)
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

/// Refuse a Petal whose route crate pins the SDK differently from `[sdk]` in
/// the build configuration.
///
/// Generated route components build against `[sdk]`. The route crate's shared
/// code and host-side tests build against its own manifest and lockfile. When
/// they disagree, components and tests run against different SDKs and an SDK
/// upgrade is half applied. This checks, against `[sdk]`:
///
/// - every SDK dependency of the route crate, in any dependency table, and any
///   `workspace = true` entry resolved from its workspace;
/// - for a git pin, every dependency on another crate from the SDK's
///   repository (such as `bloom-petal-contract`), here and in the build
///   configuration's `[dependencies]`;
/// - every lockfile entry for those packages;
/// - that no `[patch]` or `[replace]` in the route crate or its workspace
///   overrides the SDK, because generated components ignore them.
///
/// A lockfile entry with no source is accepted only while a `.cargo/config.toml`
/// patches the SDK, as the petal repository's consumer CI does.
pub fn check_sdk_pin(root: impl AsRef<Path>, config: &BuildConfig) -> Result<(), String> {
    let root = root.as_ref();
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
    let workspace = workspace_dir(root, &crate_dir, &manifest)?;
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
                && (rev != want || other_ref.is_some())
            {
                disagreements.push(format!("{label}: {source} (same repository as the SDK)"));
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
    for (path, table) in [
        (&manifest_path, Some(&manifest)),
        (&workspace_manifest_path, workspace_manifest.as_ref()),
    ] {
        for entry in table
            .map(|table| overrides(table, package))
            .into_iter()
            .flatten()
        {
            disagreements.push(format!(
                "{} {entry} overrides the SDK, which generated route components ignore",
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
        let patched = patched_by_cargo_config(root, &crate_dir, package)?;
        for entry in entries.iter().filter_map(toml::Value::as_table) {
            let Some(name) = entry.get("name").and_then(toml::Value::as_str) else {
                continue;
            };
            let version = entry.get("version").and_then(toml::Value::as_str);
            let source = entry.get("source").and_then(toml::Value::as_str);
            let matches = if name == package.as_str() {
                match source {
                    // A local path: only a pinned path or an active patch.
                    None => matches!(expected, SdkSource::Path(_)) || patched,
                    Some(source) => lock_source_matches(&expected, source, version),
                }
            } else if checked_packages.contains(name)
                && let Some(source) = source
                && source.starts_with("git+")
            {
                lock_source_matches(&expected, source, None)
            } else {
                true
            };
            if !matches {
                disagreements.push(format!(
                    "{} resolves {name} {} from {}",
                    shown(&lock_path),
                    version.unwrap_or("?"),
                    source.unwrap_or("a local path")
                ));
            }
        }
    }

    if disagreements.is_empty() {
        return Ok(());
    }
    Err(format!(
        "{package} is pinned inconsistently. [sdk] in the build configuration pins {expected}, but:\n{}\nPin every site to the same SDK source, remove any [patch] or [replace] of it, then refresh the lockfile:\n  cargo update --manifest-path {} -p {package}",
        disagreements
            .iter()
            .map(|line| format!("  - {line}"))
            .collect::<Vec<_>>()
            .join("\n"),
        shown(&workspace_manifest_path),
    ))
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
    if backup.exists() {
        eprintln!("previous output retained at {}", backup.display());
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
        replace_dir(&staging, &output).unwrap();
        assert!(output.join("new.wasm").is_file());
        assert!(!output.join("old.wasm").exists());
        assert_eq!(
            fs::read(staging.with_extension("previous").join("notes.txt")).unwrap(),
            b"preserve me"
        );
        let missing = tmp.path().join("target/missing");
        assert!(replace_dir(&missing, &output).is_err());
        assert_eq!(fs::read(output.join("new.wasm")).unwrap(), b"new component");
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

    /// The consumer CI job patches the SDK to a local checkout through
    /// `.cargo/config.toml`, which rewrites the lockfile entry to a path.
    #[test]
    fn sdk_pin_accepts_a_lockfile_patched_by_cargo_config_only() {
        let manifest = format!(
            "[dependencies]\n{}",
            git_dep("petal", "bloom-petal-sdk", REV_A)
        );
        let patched =
            "version = 4\n\n[[package]]\nname = \"bloom-petal-sdk\"\nversion = \"0.1.0\"\n";
        let (tmp, config) = pin_fixture(git_sdk(REV_A), &manifest, Some(patched));
        let error = check_sdk_pin(tmp.path(), &config).unwrap_err();
        assert!(error.contains("from a local path"), "{error}");

        fs::create_dir_all(tmp.path().join(".cargo")).unwrap();
        fs::write(
            tmp.path().join(".cargo/config.toml"),
            format!("[patch.\"{SDK_GIT}\"]\nbloom-petal-sdk = {{ path = \"/checkout/crates/petal-sdk\" }}\n"),
        )
        .unwrap();
        check_sdk_pin(tmp.path(), &config).unwrap();
    }

    #[test]
    fn sdk_pin_refuses_a_patch_or_replace_in_the_route_crate() {
        for overriding in [
            format!("[patch.\"{SDK_GIT}\"]\nbloom-petal-sdk = {{ path = \"../sdk\" }}\n"),
            "[replace]\n\"bloom-petal-sdk:0.1.0\" = { path = \"../sdk\" }\n".to_owned(),
        ] {
            let manifest = format!(
                "[dependencies]\n{}\n{overriding}",
                git_dep("petal", "bloom-petal-sdk", REV_A)
            );
            let (tmp, config) = pin_fixture(git_sdk(REV_A), &manifest, None);
            let error = check_sdk_pin(tmp.path(), &config).unwrap_err();
            assert!(error.contains("overrides the SDK"), "{error}");
        }
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
