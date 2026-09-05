**Changed**

- **The config-extension loader split into four modules.** `_load_toml_module` (with the `_toml` binding it produces) moved to `dirsql/toml_module.py`, `_load_extension_entries` to `dirsql/load_extension_entries.py` and `_has_bare_name` to `dirsql/has_bare_name.py`; `dirsql/resolve_config_extensions.py` keeps the intervene-or-not decision, `resolve_config_extension_specs`. Every function keeps its name and behavior — internal reorganisation with no change to the published API or CLI behavior. (#1079)
