//! End-to-end checks of `petal new` through the real binary: an agent that
//! scaffolds a Petal is sent to the development skill, and the CLI and the
//! generated `AGENTS.md` agree on where it is.

use std::path::Path;
use std::process::{Command, Output};

fn petal_new(name: &str, destination: &Path) -> Output {
    Command::new(env!("CARGO_BIN_EXE_petal"))
        .arg("new")
        .arg(name)
        .arg(destination)
        .output()
        .expect("run petal new")
}

fn skill_path(stdout: &str) -> &str {
    stdout
        .lines()
        .find_map(|line| line.strip_prefix("development skill: "))
        .expect("petal new names the development skill")
}

#[test]
fn new_points_the_scaffold_at_the_development_skill() {
    let tmp = tempfile::tempdir().unwrap();
    let destination = tmp.path().join("demo");
    let output = petal_new("demo", &destination);
    assert!(
        output.status.success(),
        "petal new failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8(output.stdout).unwrap();
    let path = Path::new(skill_path(&stdout));
    assert_eq!(
        path,
        destination.join(".agents/skills/bloom-petal-development/SKILL.md")
    );
    let skill = std::fs::read_to_string(path).unwrap();
    assert_eq!(skill, bloom_petal_contract::PETAL_DEVELOPMENT_SKILL);
    // The Agent Skills format requires the directory to carry the skill's name.
    assert_eq!(
        path.parent().and_then(Path::file_name).unwrap(),
        "bloom-petal-development"
    );
    assert!(skill.starts_with("---\nname: bloom-petal-development\n"));

    // The scaffold's operating contract sends agents to the same place.
    let agents = std::fs::read_to_string(destination.join("AGENTS.md")).unwrap();
    assert!(agents.contains("`.agents/skills/bloom-petal-development/SKILL.md`"));
    assert!(!agents.contains("/master/"));
    assert!(!agents.contains("{{"), "unrendered placeholder:\n{agents}");
}

#[test]
fn new_refuses_an_existing_destination() {
    let tmp = tempfile::tempdir().unwrap();
    let destination = tmp.path().join("demo");
    assert!(petal_new("demo", &destination).status.success());
    let agents = std::fs::read(destination.join("AGENTS.md")).unwrap();

    let again = petal_new("demo", &destination);
    assert!(!again.status.success());
    assert!(
        String::from_utf8_lossy(&again.stderr).contains("destination already exists"),
        "{}",
        String::from_utf8_lossy(&again.stderr)
    );
    // The refusal leaves the existing scaffold untouched.
    assert_eq!(
        std::fs::read(destination.join("AGENTS.md")).unwrap(),
        agents
    );
}

fn petal(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_petal"))
        .args(args)
        .output()
        .expect("run petal")
}

fn files_under(dir: &Path, out: &mut Vec<std::path::PathBuf>) {
    for entry in std::fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            files_under(&path, out);
        } else {
            out.push(path);
        }
    }
}

/// Whether `text` holds a full 40-character commit, as a separate pin would.
fn names_a_commit(text: &str) -> bool {
    text.split(|c: char| !c.is_ascii_hexdigit())
        .any(|run| run.len() == 40)
}

#[test]
fn new_pins_the_sdk_release_of_this_cli_and_nothing_else() {
    let tmp = tempfile::tempdir().unwrap();
    let destination = tmp.path().join("demo");
    assert!(petal_new("demo", &destination).status.success());

    let manifest = std::fs::read_to_string(destination.join("route/Cargo.toml")).unwrap();
    let pin = format!(
        "git = \"https://github.com/bloom-directory/petal\", tag = \"v{}\"",
        env!("CARGO_PKG_VERSION")
    );
    assert!(manifest.contains(&pin), "{manifest}");

    // No other file pins a commit or names the SDK's version: the scripts and
    // workflows follow route/Cargo.lock. The release workflow's `uses:` is the
    // one other reference, and it selects only the workflow.
    let mut files = Vec::new();
    files_under(&destination, &mut files);
    for file in &files {
        let relative = file.strip_prefix(&destination).unwrap();
        let body = std::fs::read_to_string(file).unwrap();
        let rendered = body.replace("${{", "");
        assert!(
            !rendered.contains("{{"),
            "unrendered placeholder in {relative:?}"
        );
        // Manifests, scripts and workflows are where a separate pin would go.
        if relative
            .extension()
            .and_then(|extension| extension.to_str())
            .is_some_and(|extension| ["toml", "sh", "yml"].contains(&extension))
        {
            assert!(
                !body.contains("petal-tooling-ref") && !names_a_commit(&body),
                "{relative:?} pins the tooling separately"
            );
        }
    }
    for expected in [
        ".gitignore",
        ".github/workflows/ci.yml",
        ".github/workflows/release.yml",
        "scripts/petal.sh",
        "scripts/build.sh",
        "route/files/account/[wallet]/[index]/address.json.rs",
    ] {
        assert!(destination.join(expected).is_file(), "missing {expected}");
    }

    #[cfg(unix)]
    for script in ["scripts/petal.sh", "scripts/build.sh"] {
        use std::os::unix::fs::PermissionsExt;
        let mode = std::fs::metadata(destination.join(script))
            .unwrap()
            .permissions()
            .mode();
        assert_eq!(mode & 0o111, 0o111, "{script} is not executable");
    }
}

#[test]
fn version_and_inspect_name_the_sdk_release() {
    let version = env!("CARGO_PKG_VERSION");
    let output = petal(&["--version"]);
    assert!(output.status.success());
    assert_eq!(
        String::from_utf8(output.stdout).unwrap().trim(),
        format!("petal {version}")
    );

    let output = petal(&["inspect"]);
    assert!(output.status.success());
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(
        stdout.contains(&format!("version: {version}\n")),
        "{stdout}"
    );
    assert!(
        stdout.contains(&format!(
            "sdk: bloom-petal-sdk git https://github.com/bloom-directory/petal tag v{version}\n"
        )),
        "{stdout}"
    );
    assert!(
        stdout.contains(&format!(
            "wasm_tools: {}\n",
            bloom_petal_contract::WASM_TOOLS_VERSION
        )),
        "{stdout}"
    );
}

#[test]
fn skill_refreshes_a_petal_copy() {
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp
        .path()
        .join(bloom_petal_contract::PETAL_DEVELOPMENT_SKILL_PATH);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(&path, "an older skill\n").unwrap();
    let output = petal(&["skill", "--root", tmp.path().to_str().unwrap()]);
    assert!(output.status.success());
    assert_eq!(
        std::fs::read_to_string(&path).unwrap(),
        bloom_petal_contract::PETAL_DEVELOPMENT_SKILL
    );

    let output = petal(&["skill"]);
    assert_eq!(
        String::from_utf8(output.stdout).unwrap(),
        bloom_petal_contract::PETAL_DEVELOPMENT_SKILL
    );
}
