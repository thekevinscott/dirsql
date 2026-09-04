**Changed**

- **The launcher's argv config-path scan moved to its own module.** `config_paths_from_argv` now lives in `dirsql/cli/config_paths_from_argv.py`; `with_resolved_extensions` imports it. Internal reorganisation with no change to the published API or CLI behavior, mirroring the TypeScript launcher. (#1042)
