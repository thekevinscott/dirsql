### Core: `FunctionSpec` gains `batch`; `ConfigError::InvalidFunctionBatch` added

#### Summary

`config::FunctionSpec` has a new `batch: Option<usize>` field and
`config::ConfigError` a new `InvalidFunctionBatch` variant, carrying the
`[[dirsql.function]]` key `batch = N`. Only Rust callers that build a
`FunctionSpec` by struct literal or match `ConfigError` exhaustively are
affected; the Python and TypeScript SDKs and the CLI read the key from the
config file and see only the error text.

#### Required changes

| Surface | Before | After |
| ------- | ------ | ----- |
| `FunctionSpec { .. }` literal | No `batch` field | Add `batch: None` (or `Some(n)`) |
| Exhaustive `match` on `ConfigError` | No `InvalidFunctionBatch` arm | Add `ConfigError::InvalidFunctionBatch { name, value }` |

#### Deprecations removed

_None._

#### Behavior changes without code changes

_None._ A function without `batch` is called exactly as before.

#### Verification

```bash
cd "$(mktemp -d)"
printf '[[dirsql.function]]\nname = "f"\nargs = [1]\ncommand = "cat"\nbatch = 0\n' > .dirsql.toml
dirsql -c .dirsql.toml "SELECT 1"
# expected on stderr: dirsql query: failed to load config: config error: [[dirsql.function]] 'f': 'batch' must be a positive integer, got 0
```
