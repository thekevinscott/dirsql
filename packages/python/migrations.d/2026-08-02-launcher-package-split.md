# Launcher and shared resolvers moved to top-level packages

#### Summary

The CLI launcher moved from `dirsql.cli` to a top-level `dirsql_cli` package,
and the two resolver modules it shares with the SDK moved from `dirsql.*` to a
top-level `_dirsql_shared` package. Importing `dirsql.cli.main` executed
`dirsql/__init__.py` first, which dlopened the compiled extension and imported
asyncio on every CLI invocation only to discard them at `os.execv` (#718).

The SDK's public API is untouched. Only import paths that were never exported
from `dirsql.__all__` changed.

#### Required changes

| Surface | Prior call site | New call site |
|---|---|---|
| Launcher entry point | `dirsql.cli.main:main` | `dirsql_cli.main:main` |
| Launcher submodules | `dirsql.cli.<name>` | `dirsql_cli.<name>` |
| Extension-path resolver | `dirsql.resolve_extension` | `_dirsql_shared.resolve_extension` |
| Config-extension resolver | `dirsql.resolve_config_extensions` | `_dirsql_shared.resolve_config_extensions` |

The console script is unaffected -- `dirsql` is still installed by the same
distribution and behaves identically. Only code that imported these modules
directly needs updating; none of them appears in `dirsql.__all__` or the docs.

Note that `import dirsql; dirsql.resolve_extension` previously resolved without
an explicit import, because the eager barrel imported the submodule as a side
effect and Python bound it on the package. That no longer happens.

#### Deprecations removed

_None._

#### Behavior changes without code changes

- `dirsql` (the CLI): the launcher no longer imports the `dirsql` package, the
  `_dirsql` extension, or asyncio. Output, exit codes, argv forwarding, plugin
  discovery and extension resolution are unchanged; only startup work is
  removed.
- Bundled binary location inside the wheel: previously `dirsql/_binary/`, now
  `dirsql_cli/_binary/`. Internal to the wheel; `binary_path()` resolves it.

#### Verification

```bash
python -c "import sys, dirsql_cli.main; print(sorted(m for m in ('dirsql', 'dirsql._dirsql', 'asyncio') if m in sys.modules))"
# expected: []

python -c "from dirsql import DirSQL, Table, RowEvent, __version__; print(__version__)"
# expected: the installed version, e.g. 0.2.7

dirsql --version
# expected: dirsql <version>
```
