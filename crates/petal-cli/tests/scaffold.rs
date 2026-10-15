//! End-to-end checks of `petal new` through the real binary: an agent that
//! scaffolds a Petal is sent to the development skill, and the CLI and the
//! generated `AGENTS.md` agree on where it is.

use std::path::Path;
use std::process::{Command, Output};

const SKILL_URL_PREFIX: &str = "https://github.com/bloom-directory/petal/tree/";
const SKILL_DIR: &str = "skills/bloom-petal-development";

fn petal_new(name: &str, destination: &Path) -> Output {
    Command::new(env!("CARGO_BIN_EXE_petal"))
        .arg("new")
        .arg(name)
        .arg(destination)
        .output()
        .expect("run petal new")
}

fn skill_url(stdout: &str) -> &str {
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
    let url = skill_url(&stdout);

    // The link names this repository's skill directory, which exists here.
    let (branch, path) = url
        .strip_prefix(SKILL_URL_PREFIX)
        .and_then(|rest| rest.split_once('/'))
        .unwrap_or_else(|| panic!("unexpected skill URL {url}"));
    assert_eq!(branch, "master", "{url}");
    assert_eq!(path, SKILL_DIR, "{url}");
    let repo = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let skill = std::fs::read_to_string(repo.join(SKILL_DIR).join("SKILL.md")).unwrap();
    assert_eq!(skill, bloom_petal_contract::PETAL_DEVELOPMENT_SKILL);

    // The scaffold's operating contract sends agents to the same place.
    let agents = std::fs::read_to_string(destination.join("AGENTS.md")).unwrap();
    assert!(
        agents.lines().any(|line| line.trim() == url),
        "AGENTS.md does not link {url}:\n{agents}"
    );
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
