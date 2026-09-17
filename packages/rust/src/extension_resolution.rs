//! Planning the resolution of `[[dirsql.extension]]` entries.
//!
//! Each launcher used to carry its own port of this: read the configs, decide
//! whether any entry names a package rather than a file, resolve the rest
//! against the config's directory. Only "where is package `X` installed on
//! this host" is genuinely host-specific (`importlib.util.find_spec` /
//! `require.resolve`), so that stays with the launcher and comes back here as
//! a set of directories.
//!
//! The suffix list is a parameter because it differs per host: Python counts
//! `.pyd` as a loadable, Node counts `.node`.

use std::fmt;
use std::path::{Path, PathBuf};

use crate::config::load_config_str;

/// One config file's contents, or `None` when it is missing or unreadable.
///
/// `path` must be absolute: entry paths and the bare-name shadow probe resolve
/// against its parent directory.
#[derive(Debug, Clone, Copy)]
pub struct ConfigSource<'a> {
    pub path: &'a Path,
    pub contents: Option<&'a str>,
}

/// One planned extension entry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PlanEntry {
    /// A concrete path, already absolute against its config's directory.
    Literal {
        path: PathBuf,
        entrypoint: Option<String>,
    },
    /// A bare package name. Use `shadow` when it is a file on disk; otherwise
    /// locate the package on the host and hand its directories, plus every
    /// file found under them, to [`select_loadable`].
    Package {
        name: String,
        shadow: PathBuf,
        entrypoint: Option<String>,
    },
}

/// Why a package's loadable file could not be picked.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SelectError {
    NotFound {
        name: String,
        suffixes: Vec<String>,
        dirs: Vec<PathBuf>,
    },
    Ambiguous {
        name: String,
        matches: Vec<PathBuf>,
    },
}

fn join_display(paths: &[PathBuf], sep: &str) -> String {
    paths
        .iter()
        .map(|p| p.display().to_string())
        .collect::<Vec<_>>()
        .join(sep)
}

impl fmt::Display for SelectError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SelectError::NotFound {
                name,
                suffixes,
                dirs,
            } => {
                let patterns = suffixes
                    .iter()
                    .map(|s| format!("*{s}"))
                    .collect::<Vec<_>>()
                    .join(" / ");
                write!(
                    f,
                    "no loadable extension file ({patterns}) found in package '{name}' (searched {})",
                    join_display(dirs, ", ")
                )
            }
            SelectError::Ambiguous { name, matches } => write!(
                f,
                "multiple loadable extension files found in package '{name}': {}; disambiguate with a literal path",
                join_display(matches, ", ")
            ),
        }
    }
}

impl std::error::Error for SelectError {}

/// True when `path` names a package rather than a file.
///
/// Both separators count regardless of host, so a Windows-style path is a path
/// everywhere.
pub fn is_bare_name(path: &str, suffixes: &[&str]) -> bool {
    todo!()
}

/// Plan the extension entries of several configs, in order.
///
/// `None` means "do not intervene": no config names an extension by bare
/// package name, so the core's own config loading (and its error reporting)
/// handles every entry. Otherwise **every** config's entries are planned, each
/// against its own directory, concatenated in `configs` order — once the
/// caller suppresses the core's config-extension loading it must supply the
/// literal entries too.
///
/// A config whose contents are absent or do not parse is skipped rather than
/// reported, leaving the error to the core.
pub fn plan_config_extensions(
    configs: &[ConfigSource<'_>],
    suffixes: &[&str],
) -> Option<Vec<PlanEntry>> {
    todo!()
}

/// Pick the single loadable file for package `name` out of a host's listing.
///
/// `dirs` are the package's directories (used only for the not-found message);
/// `candidates` is every file found under them. Zero matches and more than one
/// are both errors — the user must disambiguate with a literal path.
pub fn select_loadable(
    name: &str,
    dirs: &[PathBuf],
    candidates: &[PathBuf],
    suffixes: &[&str],
) -> Result<PathBuf, SelectError> {
    todo!()
}

#[cfg(test)]
mod tests {
    use super::{ConfigSource, PlanEntry, SelectError, is_bare_name, plan_config_extensions, select_loadable};
    use std::path::{Path, PathBuf};

    const PY: &[&str] = &[".so", ".dylib", ".dll", ".pyd"];
    const NODE: &[&str] = &[".so", ".dylib", ".dll", ".node"];

    fn source<'a>(path: &'a Path, contents: &'a str) -> ConfigSource<'a> {
        ConfigSource {
            path,
            contents: Some(contents),
        }
    }

    #[test]
    fn a_value_with_no_separator_and_no_loadable_suffix_is_a_bare_name() {
        assert!(is_bare_name("sqlite_vec", PY));
        assert!(is_bare_name("vec0", NODE));
    }

    #[test]
    fn a_value_carrying_either_separator_is_a_path() {
        assert!(!is_bare_name("ext/vec0", PY));
        assert!(!is_bare_name("ext\\vec0", PY));
        assert!(!is_bare_name("/abs/vec0", PY));
    }

    #[test]
    fn a_value_ending_in_a_loadable_suffix_is_a_path() {
        for suffix in [".so", ".dylib", ".dll"] {
            assert!(!is_bare_name(&format!("vec0{suffix}"), PY));
            assert!(!is_bare_name(&format!("vec0{suffix}"), NODE));
        }
    }

    #[test]
    fn the_suffix_list_is_the_hosts_own() {
        assert!(!is_bare_name("vec0.pyd", PY));
        assert!(is_bare_name("vec0.pyd", NODE));
        assert!(!is_bare_name("vec0.node", NODE));
        assert!(is_bare_name("vec0.node", PY));
    }

    #[test]
    fn configs_using_only_literal_paths_are_left_to_the_core() {
        let path = Path::new("/project/.dirsql.toml");
        let config = "[[dirsql.extension]]\npath = \"ext/vec0.so\"\n";
        assert_eq!(plan_config_extensions(&[source(path, config)], PY), None);
    }

    #[test]
    fn a_config_with_no_extension_array_is_left_to_the_core() {
        let path = Path::new("/project/.dirsql.toml");
        assert_eq!(
            plan_config_extensions(&[source(path, "[dirsql]\nignore = [\"*.tmp\"]\n")], PY),
            None
        );
    }

    #[test]
    fn an_absent_or_unparseable_config_is_skipped_rather_than_reported() {
        let path = Path::new("/project/.dirsql.toml");
        let missing = ConfigSource {
            path,
            contents: None,
        };
        assert_eq!(plan_config_extensions(&[missing], PY), None);
        assert_eq!(
            plan_config_extensions(&[source(path, "not valid = [")], PY),
            None
        );
    }

    #[test]
    fn a_bare_name_plans_a_package_lookup_shadowed_by_the_configs_own_directory() {
        let path = Path::new("/project/conf/.dirsql.toml");
        let plan = plan_config_extensions(
            &[source(path, "[[dirsql.extension]]\npath = \"sqlite_vec\"\n")],
            PY,
        )
        .expect("a bare name makes the SDK intervene");

        assert_eq!(
            plan,
            vec![PlanEntry::Package {
                name: "sqlite_vec".into(),
                shadow: PathBuf::from("/project/conf/sqlite_vec"),
                entrypoint: None,
            }]
        );
    }

    #[test]
    fn intervening_plans_the_literal_entries_too_and_keeps_config_order() {
        let first = Path::new("/project/one/.dirsql.toml");
        let second = Path::new("/project/two/.dirsql.toml");
        let plan = plan_config_extensions(
            &[
                source(
                    first,
                    "[[dirsql.extension]]\npath = \"ext/vec0.so\"\nentrypoint = \"sqlite3_vec_init\"\n",
                ),
                source(second, "[[dirsql.extension]]\npath = \"sqlite_vec\"\n"),
            ],
            PY,
        )
        .expect("a bare name anywhere makes the SDK intervene for the whole set");

        assert_eq!(
            plan,
            vec![
                PlanEntry::Literal {
                    path: PathBuf::from("/project/one/ext/vec0.so"),
                    entrypoint: Some("sqlite3_vec_init".into()),
                },
                PlanEntry::Package {
                    name: "sqlite_vec".into(),
                    shadow: PathBuf::from("/project/two/sqlite_vec"),
                    entrypoint: None,
                },
            ]
        );
    }

    #[test]
    fn an_absolute_entry_path_is_left_alone() {
        let path = Path::new("/project/.dirsql.toml");
        let plan = plan_config_extensions(
            &[source(
                path,
                "[[dirsql.extension]]\npath = \"/opt/vec0.so\"\n\n[[dirsql.extension]]\npath = \"sqlite_vec\"\n",
            )],
            PY,
        )
        .expect("intervening");

        assert_eq!(
            plan[0],
            PlanEntry::Literal {
                path: PathBuf::from("/opt/vec0.so"),
                entrypoint: None,
            }
        );
    }

    #[test]
    fn select_loadable_returns_the_single_matching_file() {
        let dir = PathBuf::from("/site-packages/sqlite_vec");
        let found = select_loadable(
            "sqlite_vec",
            &[dir.clone()],
            &[
                dir.join("__init__.py"),
                dir.join("vec0.so"),
                dir.join("README.md"),
            ],
            PY,
        );
        assert_eq!(found, Ok(dir.join("vec0.so")));
    }

    #[test]
    fn select_loadable_dedupes_and_sorts_before_counting() {
        let dir = PathBuf::from("/pkg");
        let found = select_loadable(
            "vec",
            &[dir.clone()],
            &[dir.join("vec0.so"), dir.join("vec0.so")],
            PY,
        );
        assert_eq!(found, Ok(dir.join("vec0.so")));
    }

    #[test]
    fn select_loadable_reports_every_searched_directory_when_nothing_matches() {
        let dirs = vec![PathBuf::from("/a"), PathBuf::from("/b")];
        let err = select_loadable("vec", &dirs, &[PathBuf::from("/a/vec.py")], PY)
            .expect_err("no loadable file");
        assert_eq!(
            err.to_string(),
            "no loadable extension file (*.so / *.dylib / *.dll / *.pyd) found in package 'vec' (searched /a, /b)"
        );
    }

    #[test]
    fn select_loadable_rejects_an_ambiguous_package_in_sorted_order() {
        let dir = PathBuf::from("/pkg");
        let err = select_loadable(
            "vec",
            &[dir.clone()],
            &[dir.join("z.so"), dir.join("a.so")],
            PY,
        )
        .expect_err("two loadable files");
        assert_eq!(
            err,
            SelectError::Ambiguous {
                name: "vec".into(),
                matches: vec![dir.join("a.so"), dir.join("z.so")],
            }
        );
        assert_eq!(
            err.to_string(),
            "multiple loadable extension files found in package 'vec': /pkg/a.so, /pkg/z.so; disambiguate with a literal path"
        );
    }
}
