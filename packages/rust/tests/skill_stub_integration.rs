//! Integration tests for the `/dirsql` agent skill at
//! `.claude/skills/dirsql/SKILL.md`. The skill must stay a stub that injects
//! `dirsql context`: any usage guidance written into it would drift from the
//! binary that runs the query.

#![cfg(feature = "cli")]

use assert_cmd::prelude::*;

const INJECTED_COMMAND: &str = "uvx dirsql context";

fn skill() -> String {
    let path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../.claude/skills/dirsql/SKILL.md"
    );
    std::fs::read_to_string(path)
        .unwrap_or_else(|e| panic!("reading {path}: {e}"))
        .replace("\r\n", "\n")
}

fn frontmatter_and_body(skill: &str) -> (&str, &str) {
    let rest = skill
        .strip_prefix("---\n")
        .expect("SKILL.md must open with a `---` frontmatter fence");
    rest.split_once("\n---\n")
        .expect("SKILL.md frontmatter must close with a `---` fence")
}

fn injected_commands(body: &str) -> Vec<&str> {
    body.match_indices("!`")
        .map(|(start, _)| {
            let command = &body[start + 2..];
            &command[..command.find('`').expect("unterminated !` injection")]
        })
        .collect()
}

#[test]
fn the_frontmatter_names_the_skill_and_describes_its_trigger() {
    let skill = skill();
    let (frontmatter, _) = frontmatter_and_body(&skill);
    assert!(
        frontmatter.lines().any(|line| line == "name: dirsql"),
        "frontmatter must declare `name: dirsql`:\n{frontmatter}"
    );
    assert!(
        frontmatter
            .lines()
            .any(|line| line.starts_with("description: ")),
        "frontmatter must carry a description:\n{frontmatter}"
    );
}

#[test]
fn the_body_injects_exactly_uvx_dirsql_context() {
    let skill = skill();
    let (_, body) = frontmatter_and_body(&skill);
    assert_eq!(injected_commands(body), vec![INJECTED_COMMAND]);
    assert!(
        !body.contains("```!"),
        "the body must not inject a multi-line command block"
    );
}

#[test]
fn the_body_falls_back_to_running_the_command_by_hand() {
    let skill = skill();
    let (_, body) = frontmatter_and_body(&skill);
    let injection = format!("!`{INJECTED_COMMAND}`");
    let after_injection = &body[body.find(&injection).unwrap() + injection.len()..];
    assert!(
        after_injection.contains(&format!("run `{INJECTED_COMMAND}`")),
        "a fallback telling the agent to run `{INJECTED_COMMAND}` must follow the injection:\n{body}"
    );
}

#[test]
fn the_body_carries_no_usage_guidance_that_could_drift() {
    let skill = skill();
    let (_, body) = frontmatter_and_body(&skill);
    for forbidden in [
        "```",
        "|",
        "'./",
        "'**",
        "SELECT",
        "select ",
        "FROM",
        "->>",
        "-c ",
        "--on-file",
        "dirsql-plugin",
        ".dirsql.toml",
    ] {
        assert!(
            !body.contains(forbidden),
            "SKILL.md body contains `{forbidden}`; usage guidance belongs in `dirsql context`:\n{body}"
        );
    }
}

#[test]
fn the_injected_command_runs() {
    let args: Vec<&str> = INJECTED_COMMAND
        .strip_prefix("uvx dirsql ")
        .unwrap()
        .split(' ')
        .collect();
    let out = std::process::Command::cargo_bin("dirsql")
        .expect("`dirsql` binary must be built with --features cli")
        .args(&args)
        .output()
        .expect("spawning dirsql failed");
    assert!(
        out.status.success(),
        "`dirsql {}` failed: stderr={}",
        args.join(" "),
        String::from_utf8_lossy(&out.stderr)
    );
}
