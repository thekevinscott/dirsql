**Changed**

- **The extension-path resolver split into four modules.** `_platform_patterns` moved to `dirsql/platform_patterns.py`, `is_bare_name` (with `_LOADABLE_SUFFIXES`) to `dirsql/is_bare_name.py`, and `_resolve_package` to `dirsql/resolve_package.py`; `dirsql/resolve_extension.py` keeps the ordered probe, `resolve_extension_path`. Every function keeps its name and behavior — internal reorganisation with no change to the published API or CLI behavior. (#1066)
