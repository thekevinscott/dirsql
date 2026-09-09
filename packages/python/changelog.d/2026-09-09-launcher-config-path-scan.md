**Changed**

- **The launcher's config-path scan now runs in the shared core.** Before clap sees argv, the pip launcher still walks it for every `--config` / `-c` value (all five spellings) so a bare package-name `[[dirsql.extension]]` entry can be resolved through `importlib` — but the walk itself is now `dirsql::launcher::config_paths_from_argv`, reached through the private `dirsql._dirsql.config_paths_from_argv` binding; the Python copy is deleted. No user-observable change: same flags, same paths, same default of `./.dirsql.toml`. (#1078, #1072)
