use std::fs;
use std::path::{Path, PathBuf};

use clap::{Parser, Subcommand};

#[derive(Parser, Debug)]
#[command(name = "petal", about = "Build and inspect Bloom Petals", version)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand, Debug)]
enum Command {
    /// Compile route sources into a Petal package tree.
    Build {
        #[arg(long, default_value = "petal-build.toml")]
        config: PathBuf,
        #[arg(long, default_value = ".")]
        root: PathBuf,
    },
    /// Build current routes and create a deterministic runtime archive.
    Package {
        #[arg(long, default_value = "petal-build.toml")]
        config: PathBuf,
        #[arg(long, default_value = ".")]
        root: PathBuf,
        #[arg(long)]
        out: PathBuf,
        /// Package the components a Petal's own build command produced,
        /// checking their capabilities, instead of rebuilding them in this
        /// environment, which lacks any compile-time values that command set.
        #[arg(long)]
        no_build: bool,
    },
    /// Check route architecture and generated capabilities without rebuilding.
    Check {
        #[arg(long, default_value = "petal-build.toml")]
        config: PathBuf,
        #[arg(long, default_value = ".")]
        root: PathBuf,
    },
    /// Print this release's version, SDK pin, contract, WIT digest and
    /// wasm-tools version.
    Inspect,
    /// Scaffold a Rust route Petal pinned to this release's SDK.
    New { name: String, destination: PathBuf },
    /// Print the development skill of this release, or write it into a Petal.
    Skill {
        /// Write it to `.agents/skills/bloom-petal-development/SKILL.md` under
        /// this Petal root instead of printing it.
        #[arg(long)]
        root: Option<PathBuf>,
    },
}

fn main() {
    if let Err(error) = run() {
        eprintln!("error: {error}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), String> {
    match Cli::parse().command {
        Command::Build { config, root } => {
            let config = resolve_config(&root, &config);
            let config = bloom_petal_builder::load_config(&config)?;
            let report = bloom_petal_builder::build(&root, &config)?;
            warn_stale_skill(&root);
            println!(
                "{}",
                serde_json::to_string_pretty(&report).map_err(|error| error.to_string())?
            );
        }
        Command::Package {
            config,
            root,
            out,
            no_build,
        } => {
            let config = resolve_config(&root, &config);
            let config = bloom_petal_builder::load_config(&config)?;
            if no_build {
                bloom_petal_builder::check_caps(&root, &config)?;
            } else {
                bloom_petal_builder::build(&root, &config)?;
            }
            let report = bloom_petal_builder::package(&root, &config, &out)?;
            println!(
                "{}",
                serde_json::to_string_pretty(&report).map_err(|error| error.to_string())?
            );
        }
        Command::Check { config, root } => {
            let config = resolve_config(&root, &config);
            let config = bloom_petal_builder::load_config(&config)?;
            let report = bloom_petal_builder::check_caps(&root, &config)?;
            warn_stale_skill(&root);
            println!(
                "{}",
                serde_json::to_string_pretty(&report).map_err(|error| error.to_string())?
            );
        }
        Command::Inspect => {
            let version = bloom_petal_builder::TOOLING_VERSION;
            println!("version: {version}");
            println!(
                "sdk: bloom-petal-sdk git {} tag v{version}",
                bloom_petal_builder::PETAL_REPOSITORY
            );
            println!("contract: {}", bloom_petal_contract::ROUTE_PACKAGE);
            println!("wit_digest: {}", bloom_petal_contract::wit_digest());
            println!("wasm_tools: {}", bloom_petal_contract::WASM_TOOLS_VERSION);
        }
        Command::New { name, destination } => scaffold(&name, &destination)?,
        Command::Skill { root: None } => {
            print!("{}", bloom_petal_contract::PETAL_DEVELOPMENT_SKILL)
        }
        Command::Skill { root: Some(root) } => {
            let path = write_skill(&root)?;
            println!("development skill: {}", path.display());
        }
    }
    Ok(())
}

fn resolve_config(root: &Path, config: &Path) -> PathBuf {
    if config.is_absolute() {
        config.to_owned()
    } else {
        root.join(config)
    }
}

fn scaffold(name: &str, destination: &Path) -> Result<(), String> {
    validate_name(name)?;
    if destination.exists() {
        return Err(format!(
            "destination already exists: {}",
            destination.display()
        ));
    }
    let crate_name = name.replace('_', "-").to_ascii_lowercase();
    for (relative, template) in bloom_petal_contract::RUST_TEMPLATE_FILES {
        let path = destination.join(relative);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)
                .map_err(|error| format!("create {}: {error}", parent.display()))?;
        }
        let body = template
            .replace("{{petal-name}}", name)
            .replace("{{crate-name}}", &crate_name)
            .replace("{{sdk-version}}", bloom_petal_builder::TOOLING_VERSION)
            .replace("petal-template", &crate_name);
        fs::write(&path, body).map_err(|error| format!("write {}: {error}", path.display()))?;
        if relative.ends_with(".sh") {
            make_executable(&path)?;
        }
    }
    let skill = write_skill(destination)?;
    println!("created {}", destination.display());
    println!("development skill: {}", skill.display());
    Ok(())
}

fn write_skill(root: &Path) -> Result<PathBuf, String> {
    let skill = root.join(bloom_petal_contract::PETAL_DEVELOPMENT_SKILL_PATH);
    if let Some(parent) = skill.parent() {
        fs::create_dir_all(parent)
            .map_err(|error| format!("create {}: {error}", parent.display()))?;
    }
    fs::write(&skill, bloom_petal_contract::PETAL_DEVELOPMENT_SKILL)
        .map_err(|error| format!("write {}: {error}", skill.display()))?;
    Ok(skill)
}

/// A Petal's copy of the skill is packaged with it, so a stale copy ships
/// guidance for an older SDK. Say so, without failing the build.
fn warn_stale_skill(root: &Path) {
    let path = root.join(bloom_petal_contract::PETAL_DEVELOPMENT_SKILL_PATH);
    if let Ok(skill) = fs::read_to_string(&path) {
        if skill != bloom_petal_contract::PETAL_DEVELOPMENT_SKILL {
            eprintln!(
                "warning: {} is not the development skill of petal {}; refresh it with `petal skill --root {}`",
                path.display(),
                bloom_petal_builder::TOOLING_VERSION,
                root.display()
            );
        }
    }
}

#[cfg(unix)]
fn make_executable(path: &Path) -> Result<(), String> {
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(path, fs::Permissions::from_mode(0o755))
        .map_err(|error| format!("make {} executable: {error}", path.display()))
}

#[cfg(not(unix))]
fn make_executable(_path: &Path) -> Result<(), String> {
    Ok(())
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
