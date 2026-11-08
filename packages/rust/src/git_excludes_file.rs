use std::ffi::OsString;
use std::fs;
use std::path::{Path, PathBuf};

/// The global ignore file git reads in this environment: `core.excludesFile`
/// from the system and global configs, else `$XDG_CONFIG_HOME/git/ignore`.
/// The file may not exist.
pub(crate) fn global_excludes_path() -> Option<PathBuf> {
    excludes_path_in(&set_var)
}

/// The variable `key`, unless unset or empty, as git treats both alike.
fn set_var(key: &str) -> Option<OsString> {
    std::env::var_os(key).filter(|value| !value.is_empty())
}

/// `global_excludes_path`, reading each variable through `env`.
fn excludes_path_in(env: &dyn Fn(&str) -> Option<OsString>) -> Option<PathBuf> {
    // Git for Windows resolves `~` from `HOME` before the profile directory.
    let home = env("HOME")
        .or_else(|| env("USERPROFILE"))
        .map(PathBuf::from);
    let xdg = env("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .or_else(|| home.as_ref().map(|home| home.join(".config")));
    let mut configs = vec![
        env("GIT_CONFIG_SYSTEM").map_or_else(|| PathBuf::from("/etc/gitconfig"), PathBuf::from),
    ];
    match env("GIT_CONFIG_GLOBAL") {
        Some(global) => configs.push(PathBuf::from(global)),
        None => {
            configs.extend(xdg.as_ref().map(|xdg| xdg.join("git").join("config")));
            configs.extend(home.as_ref().map(|home| home.join(".gitconfig")));
        }
    }
    // Git reads the system config first, so a later file overrides it.
    configs
        .iter()
        .rev()
        .filter_map(|config| fs::read(config).ok())
        .find_map(|bytes| excludes_file_value(&String::from_utf8_lossy(&bytes)))
        .map(|value| expand_home(&value, home.as_deref()))
        .or_else(|| xdg.map(|xdg| xdg.join("git").join("ignore")))
}

/// The last `core.excludesFile` value set in the git config `text`.
fn excludes_file_value(text: &str) -> Option<String> {
    let mut in_core = false;
    let mut value = None;
    for line in text.lines().map(str::trim_start) {
        if let Some(header) = line.strip_prefix('[') {
            in_core = header
                .split(']')
                .next()
                .is_some_and(|name| name.trim().eq_ignore_ascii_case("core"));
        } else if let Some((key, raw)) = line.split_once('=')
            && in_core
            && key.trim().eq_ignore_ascii_case("excludesfile")
        {
            value = Some(config_value(raw));
        }
    }
    value
}

/// A git config value as git reads it: quotes removed, escapes resolved, a
/// `#` or `;` outside quotes starting a comment, and outer whitespace dropped.
fn config_value(raw: &str) -> String {
    let mut value = String::new();
    let mut kept = 0;
    let mut quoted = false;
    let mut chars = raw.trim_start().chars();
    while let Some(c) = chars.next() {
        match c {
            '"' => {
                quoted = !quoted;
                continue;
            }
            '#' | ';' if !quoted => break,
            '\\' => value.push(match chars.next() {
                Some('n') => '\n',
                Some('t') => '\t',
                Some(other) => other,
                None => break,
            }),
            c => value.push(c),
        }
        if quoted || !c.is_whitespace() {
            kept = value.len();
        }
    }
    value.truncate(kept);
    value
}

/// `value` as a path, a leading `~` naming the home directory as git's
/// does; a `~` anywhere else, such as in a Windows short name like
/// `RUNNER~1`, is literal.
fn expand_home(value: &str, home: Option<&Path>) -> PathBuf {
    let rest = value
        .strip_prefix("~/")
        .or_else(|| value.strip_prefix("~\\"))
        .or_else(|| (value == "~").then_some(""));
    match (rest, home) {
        (Some(rest), Some(home)) => home.join(rest),
        _ => PathBuf::from(value),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use tempfile::TempDir;

    fn env_of(vars: &[(&str, &Path)]) -> impl Fn(&str) -> Option<OsString> + use<> {
        let vars: HashMap<String, OsString> = vars
            .iter()
            .map(|(key, value)| (key.to_string(), value.as_os_str().to_owned()))
            .collect();
        move |key| vars.get(key).cloned()
    }

    fn write(path: &Path, text: &str) -> PathBuf {
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, text).unwrap();
        path.to_path_buf()
    }

    #[test]
    fn the_global_config_names_the_excludes_file() {
        let dir = TempDir::new().unwrap();
        let global = write(
            &dir.path().join("global"),
            "[core]\n\texcludesFile = /x/RUNNER~1/ignore\n",
        );
        let none = dir.path().join("none");
        let env = env_of(&[("GIT_CONFIG_GLOBAL", &global), ("GIT_CONFIG_SYSTEM", &none)]);
        assert_eq!(
            excludes_path_in(&env),
            Some(PathBuf::from("/x/RUNNER~1/ignore"))
        );
    }

    #[test]
    fn the_global_config_outranks_the_system_config() {
        let dir = TempDir::new().unwrap();
        let system = write(&dir.path().join("system"), "[core]\nexcludesFile = /sys\n");
        let global = write(&dir.path().join("global"), "[core]\nexcludesFile = /glob\n");
        let env = env_of(&[
            ("GIT_CONFIG_GLOBAL", &global),
            ("GIT_CONFIG_SYSTEM", &system),
        ]);
        assert_eq!(excludes_path_in(&env), Some(PathBuf::from("/glob")));
    }

    #[test]
    fn the_system_config_applies_when_no_global_config_sets_it() {
        let dir = TempDir::new().unwrap();
        let system = write(&dir.path().join("system"), "[core]\nexcludesFile = /sys\n");
        let global = write(&dir.path().join("global"), "[user]\nname = x\n");
        let env = env_of(&[
            ("GIT_CONFIG_GLOBAL", &global),
            ("GIT_CONFIG_SYSTEM", &system),
        ]);
        assert_eq!(excludes_path_in(&env), Some(PathBuf::from("/sys")));
    }

    #[test]
    fn home_gitconfig_outranks_the_xdg_config() {
        let dir = TempDir::new().unwrap();
        let home = dir.path().join("home");
        let xdg = dir.path().join("xdg");
        write(&xdg.join("git/config"), "[core]\nexcludesFile = /xdg\n");
        write(&home.join(".gitconfig"), "[core]\nexcludesFile = ~/home\n");
        let none = dir.path().join("none");
        let env = env_of(&[
            ("HOME", &home),
            ("XDG_CONFIG_HOME", &xdg),
            ("GIT_CONFIG_SYSTEM", &none),
        ]);
        assert_eq!(excludes_path_in(&env), Some(home.join("home")));
    }

    #[test]
    fn the_xdg_config_applies_without_a_home_gitconfig() {
        let dir = TempDir::new().unwrap();
        let home = dir.path().join("home");
        let xdg = dir.path().join("xdg");
        write(&xdg.join("git/config"), "[core]\nexcludesFile = /xdg\n");
        let none = dir.path().join("none");
        let env = env_of(&[
            ("HOME", &home),
            ("XDG_CONFIG_HOME", &xdg),
            ("GIT_CONFIG_SYSTEM", &none),
        ]);
        assert_eq!(excludes_path_in(&env), Some(PathBuf::from("/xdg")));
    }

    #[test]
    fn the_xdg_config_defaults_under_home() {
        let dir = TempDir::new().unwrap();
        let home = dir.path().join("home");
        write(
            &home.join(".config/git/config"),
            "[core]\nexcludesFile = /xdg\n",
        );
        let none = dir.path().join("none");
        let env = env_of(&[("HOME", &home), ("GIT_CONFIG_SYSTEM", &none)]);
        assert_eq!(excludes_path_in(&env), Some(PathBuf::from("/xdg")));
    }

    #[test]
    fn userprofile_is_home_when_home_is_unset() {
        let dir = TempDir::new().unwrap();
        let profile = dir.path().join("profile");
        write(
            &profile.join(".gitconfig"),
            "[core]\nexcludesFile = ~/ignore\n",
        );
        let none = dir.path().join("none");
        let env = env_of(&[("USERPROFILE", &profile), ("GIT_CONFIG_SYSTEM", &none)]);
        assert_eq!(excludes_path_in(&env), Some(profile.join("ignore")));
    }

    #[test]
    fn home_outranks_userprofile() {
        let dir = TempDir::new().unwrap();
        let home = dir.path().join("home");
        let profile = dir.path().join("profile");
        let none = dir.path().join("none");
        let env = env_of(&[
            ("HOME", &home),
            ("USERPROFILE", &profile),
            ("GIT_CONFIG_SYSTEM", &none),
        ]);
        assert_eq!(
            excludes_path_in(&env),
            Some(home.join(".config/git/ignore"))
        );
    }

    #[test]
    fn unset_falls_back_to_the_xdg_ignore() {
        let dir = TempDir::new().unwrap();
        let xdg = dir.path().join("xdg");
        let none = dir.path().join("none");
        let env = env_of(&[("XDG_CONFIG_HOME", &xdg), ("GIT_CONFIG_SYSTEM", &none)]);
        assert_eq!(excludes_path_in(&env), Some(xdg.join("git/ignore")));
    }

    #[test]
    fn no_home_and_no_xdg_has_no_excludes_file() {
        let dir = TempDir::new().unwrap();
        let none = dir.path().join("none");
        let env = env_of(&[("GIT_CONFIG_SYSTEM", &none)]);
        assert_eq!(excludes_path_in(&env), None);
    }

    #[test]
    fn the_process_environment_resolves_through_set_var() {
        assert_eq!(global_excludes_path(), excludes_path_in(&set_var));
        assert!(global_excludes_path().is_some());
    }

    #[test]
    fn set_var_reads_a_set_variable() {
        assert!(set_var("PATH").is_some());
        assert_eq!(set_var("DIRSQL_TEST_SURELY_UNSET_VARIABLE"), None);
    }

    #[test]
    fn only_the_core_section_sets_the_excludes_file() {
        assert_eq!(excludes_file_value("[user]\nexcludesFile = /x\n"), None);
        assert_eq!(
            excludes_file_value("[core]\nexcludesFile = /a\n[user]\nexcludesFile = /b\n"),
            Some("/a".into())
        );
    }

    #[test]
    fn a_core_subsection_is_not_core() {
        assert_eq!(
            excludes_file_value("[core \"x\"]\nexcludesFile = /x\n"),
            None
        );
    }

    #[test]
    fn section_and_key_names_ignore_case() {
        assert_eq!(
            excludes_file_value("[ CORE ]\n  EXCLUDESFILE = /x\n"),
            Some("/x".into())
        );
    }

    #[test]
    fn the_last_value_wins() {
        assert_eq!(
            excludes_file_value("[core]\nexcludesFile = /a\nexcludesFile = /b\n"),
            Some("/b".into())
        );
    }

    #[test]
    fn other_core_keys_are_not_the_excludes_file() {
        assert_eq!(excludes_file_value("[core]\nexcludes = /x\n"), None);
    }

    #[test]
    fn a_value_keeps_inner_spaces_and_drops_outer_ones() {
        assert_eq!(
            config_value("  C:/John Smith/ignore  "),
            "C:/John Smith/ignore"
        );
    }

    #[test]
    fn quotes_are_removed_and_keep_comment_marks_and_spaces() {
        assert_eq!(config_value(" \"/a #b; \" # note"), "/a #b; ");
    }

    #[test]
    fn a_comment_ends_an_unquoted_value() {
        assert_eq!(config_value("/a ; note"), "/a");
        assert_eq!(config_value("/a# note"), "/a");
    }

    #[test]
    fn escapes_resolve() {
        assert_eq!(config_value(r#"a\\b\"c\nd\te"#), "a\\b\"c\nd\te");
    }

    #[test]
    fn a_trailing_backslash_is_dropped() {
        assert_eq!(config_value("a\\"), "a");
    }

    #[test]
    fn a_leading_tilde_slash_is_home() {
        let home = Path::new("/h");
        assert_eq!(expand_home("~/x", Some(home)), home.join("x"));
        assert_eq!(expand_home("~\\x", Some(home)), home.join("x"));
        assert_eq!(expand_home("~", Some(home)), home.join(""));
    }

    #[test]
    fn a_tilde_elsewhere_is_literal() {
        let home = Path::new("/h");
        assert_eq!(
            expand_home("C:/RUNNER~1/x", Some(home)),
            PathBuf::from("C:/RUNNER~1/x")
        );
        assert_eq!(expand_home("~user/x", Some(home)), PathBuf::from("~user/x"));
    }

    #[test]
    fn a_tilde_without_a_home_is_literal() {
        assert_eq!(expand_home("~/x", None), PathBuf::from("~/x"));
    }
}
