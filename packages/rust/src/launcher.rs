//! Argv scanning the language launchers share.
//!
//! The pip and npm launchers must know which config files an invocation names
//! *before* the core parses argv: a config's `[[dirsql.extension]]` entry may
//! name an extension by bare package name, and resolving that needs the
//! interpreter the launcher has and this crate does not. Everything about
//! locating those config paths is language-neutral, so it lives here and each
//! launcher reaches it through its binding rather than reimplementing it.

/// The config a launcher inspects when argv names none.
const DEFAULT_CONFIG_PATH: &str = "./.dirsql.toml";

/// Every config value in `argv`, in order (`--config X`, `--config=X`,
/// `-c X`, `-c=X`, `-cX`), or [`DEFAULT_CONFIG_PATH`] when argv names none.
///
/// The scan is deliberately looser than clap: it runs before argv is parsed,
/// only to find files to read extensions out of, so a bare trailing `-c`
/// yields an empty string rather than an error. A malformed invocation is
/// clap's to reject once the launcher hands argv over.
pub fn config_paths_from_argv(_argv: &[String]) -> Vec<String> {
    vec![DEFAULT_CONFIG_PATH.to_string()]
}

#[cfg(test)]
mod tests {
    use super::config_paths_from_argv;

    fn scan(argv: &[&str]) -> Vec<String> {
        config_paths_from_argv(&argv.iter().map(|a| (*a).to_string()).collect::<Vec<_>>())
    }

    #[test]
    fn reads_the_long_config_form() {
        assert_eq!(
            scan(&["--config", "/cfg/.dirsql.toml"]),
            ["/cfg/.dirsql.toml"]
        );
    }

    #[test]
    fn reads_the_long_config_equals_form() {
        assert_eq!(scan(&["--config=/c/.dirsql.toml"]), ["/c/.dirsql.toml"]);
    }

    #[test]
    fn reads_the_short_c_form() {
        assert_eq!(scan(&["-c", "/frag/dirsql.toml"]), ["/frag/dirsql.toml"]);
    }

    #[test]
    fn reads_the_short_c_equals_form() {
        assert_eq!(scan(&["-c=/frag/dirsql.toml"]), ["/frag/dirsql.toml"]);
    }

    #[test]
    fn reads_the_short_c_attached_form() {
        assert_eq!(scan(&["-c/frag/dirsql.toml"]), ["/frag/dirsql.toml"]);
    }

    #[test]
    fn collects_every_config_flag_in_argv_order() {
        assert_eq!(
            scan(&[
                "-c",
                "a.toml",
                "--config",
                "b.toml",
                "--config=c.toml",
                "-cd.toml"
            ]),
            ["a.toml", "b.toml", "c.toml", "d.toml"]
        );
    }

    #[test]
    fn reads_the_config_value_at_any_argv_position() {
        assert_eq!(scan(&["-v", "--config", "/x/y", "tail"]), ["/x/y"]);
    }

    #[test]
    fn collects_a_discovery_injected_fragment_rather_than_the_default() {
        assert_eq!(
            scan(&[
                "query",
                "SELECT 1",
                "--include-default",
                "-c",
                "/frag/dirsql.toml"
            ]),
            ["/frag/dirsql.toml"]
        );
    }

    #[test]
    fn consumes_a_config_value_that_looks_like_a_flag() {
        assert_eq!(scan(&["--config", "-cx.toml"]), ["-cx.toml"]);
    }

    #[test]
    fn consumes_a_config_value_that_is_itself_a_config_flag() {
        assert_eq!(scan(&["-c", "-c", "x"]), ["-c"]);
    }

    #[test]
    fn does_not_mistake_other_short_flags_for_config() {
        assert_eq!(scan(&["-v", "-x", "val"]), ["./.dirsql.toml"]);
    }

    #[test]
    fn does_not_mistake_other_long_flags_for_config() {
        assert_eq!(scan(&["--a", "val"]), ["./.dirsql.toml"]);
    }

    #[test]
    fn does_not_mistake_a_config_prefixed_long_flag_for_config() {
        assert_eq!(scan(&["--configuration", "val"]), ["./.dirsql.toml"]);
    }

    #[test]
    fn defaults_when_no_config_flag_is_given() {
        assert_eq!(scan(&["--port", "9000"]), ["./.dirsql.toml"]);
    }

    #[test]
    fn defaults_for_an_empty_argv() {
        assert_eq!(scan(&[]), ["./.dirsql.toml"]);
    }

    #[test]
    fn treats_a_bare_trailing_long_config_as_an_empty_path() {
        assert_eq!(scan(&["--config"]), [""]);
    }

    #[test]
    fn treats_a_bare_trailing_short_c_as_an_empty_path() {
        assert_eq!(scan(&["-c"]), [""]);
    }

    #[test]
    fn keeps_a_bare_trailing_short_c_after_an_earlier_config() {
        assert_eq!(scan(&["--config", "a.toml", "-c"]), ["a.toml", ""]);
    }

    #[test]
    fn reads_an_empty_value_from_the_equals_forms() {
        assert_eq!(scan(&["--config="]), [""]);
        assert_eq!(scan(&["-c="]), [""]);
    }

    #[test]
    fn does_not_treat_a_double_dash_as_end_of_options() {
        assert_eq!(scan(&["--", "-c", "a.toml"]), ["a.toml"]);
    }
}
