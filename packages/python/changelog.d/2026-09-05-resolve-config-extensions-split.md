**Changed**

- **The config-extension resolver split into three modules.** `_resolve_entries` moved to `dirsql/resolve_entries.py` and `resolve_configs_extension_specs` to `dirsql/resolve_configs_extension_specs.py`; `dirsql/resolve_config_extensions.py` keeps the TOML loading and the single-config `resolve_config_extension_specs`. Every function keeps its name and behavior — internal reorganisation with no change to the published API or CLI behavior. (#1056)
