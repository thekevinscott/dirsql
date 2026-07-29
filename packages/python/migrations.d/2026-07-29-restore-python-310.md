### Python 3.10 support restored (#698)

#### Summary

`requires-python` is lowered from `>=3.11` back to `>=3.10`, reversing the drop
made in 0.3.6 ("Python 3.10 support dropped"). No public API changes: this is a
packaging/installability change plus the wheel-tag change that follows it.

The 0.3.6 drop was forced by release tooling, not by the codebase — putitoutthere's
multi-version wheel fan ran an `import tomllib` verify step on every row, so the
3.10 row crashed the release build. Since putitoutthere#401 a wheel detected as
Python-version-independent (dirsql's pyo3 `abi3-pyXY` feature, read off
`packages/python/Cargo.toml`) collapses to a single wheel row built on the
**newest** resolved interpreter, so that verify step never runs on 3.10 again.

#### Required changes

_None._ Every 3.11+ install keeps resolving and installing exactly as before.

| Before | After | Action |
| ------ | ----- | ------ |
| `pip install dirsql` on 3.10 → `Requires-Python >=3.11`, no candidate | resolves and installs | None — upgrading Python is no longer required. |
| Wheel filename `…-cp311-abi3-<platform>.whl` | `…-cp310-abi3-<platform>.whl` | Only if you **pin or hash exact wheel filenames** (lockfile with hashes, vendored artifact mirror, CI cache key): re-resolve so the new `cp310-abi3` name is recorded. |

#### Deprecations removed

_None._

#### Behavior changes without code changes

- Installs now succeed on CPython 3.10; the package previously refused to
  resolve there.
- The published wheel is tagged `cp310-abi3` instead of `cp311-abi3`. It is
  still one stable-ABI wheel per platform, loadable on every CPython from the
  tagged version up — 3.11 through 3.14 keep installing the same artifact shape.
- On 3.10 only, `dirsql` pulls in one new runtime dependency, `tomli>=2` (the
  backport `tomllib` was derived from). It is not installed on 3.11+.

#### Verification

```console
$ python3.10 -m pip install dirsql
$ python3.10 -c "import dirsql; print(dirsql.__version__)"
```

```python
# On 3.10 the config parser transparently uses the backport:
import dirsql.resolve_config_extensions as rce
print(rce._toml.__name__)  # -> 'tomli' on 3.10, 'tomllib' on 3.11+
```
