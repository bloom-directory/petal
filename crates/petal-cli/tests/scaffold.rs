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
