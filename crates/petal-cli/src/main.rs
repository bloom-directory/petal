use std::fs;
use std::path::{Path, PathBuf};

use clap::{Parser, Subcommand};

#[derive(Parser, Debug)]
#[command(name = "petal", about = "Build and inspect Bloom Petals")]
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
    /// Check generated route capabilities without rebuilding.
    Check {
        #[arg(long, default_value = "petal-build.toml")]
        config: PathBuf,
        #[arg(long, default_value = ".")]
        root: PathBuf,
    },
    /// Print the canonical contract version and WIT digest.
    Inspect,
    /// Scaffold a minimal Rust route Petal.
    New { name: String, destination: PathBuf },
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
            println!(
                "{}",
                serde_json::to_string_pretty(&report).map_err(|error| error.to_string())?
            );
        }
        Command::Check { config, root } => {
            let config = resolve_config(&root, &config);
            let config = bloom_petal_builder::load_config(&config)?;
            let report = bloom_petal_builder::check_caps(&root, &config)?;
            println!(
                "{}",
                serde_json::to_string_pretty(&report).map_err(|error| error.to_string())?
            );
        }
        Command::Inspect => {
            println!("contract: {}", bloom_petal_contract::ROUTE_PACKAGE);
            println!("wit_digest: {}", bloom_petal_contract::wit_digest());
        }
        Command::New { name, destination } => scaffold(&name, &destination)?,
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
            .replace("petal-template", &crate_name);
        fs::write(&path, body).map_err(|error| format!("write {}: {error}", path.display()))?;
    }
    println!("created {}", destination.display());
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
