use std::collections::{BTreeMap, BTreeSet};
use std::ffi::OsStr;
use std::fs;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use serde::{Deserialize, Serialize};
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
            println!(
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
