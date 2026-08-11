### Core: `[dirsql].hook-timeout` removed; `on-file` runs unbounded

#### Summary

The `[dirsql].hook-timeout` config key is removed (#820). `on-file` hook
runs are no longer bounded by `dirsql` at all — bounding a hook is now the
command's own job, via `timeout(1)`. Every install channel is affected
identically (`pip` / `npm` / `cargo`, CLI and HTTP server): the key is no
longer part of the schema, and a config still declaring it fails to load
with a dedicated error naming the `timeout(1)` replacement (not the generic
unknown-key error). `[[dirsql.function]]` worker calls keep their per-call
`timeout` key; when absent, the function mechanism's own 30-second default
applies (`functions::DEFAULT_FUNCTION_TIMEOUT` — a round-trip on a
persistent worker cannot be expressed with `timeout(1)`, so that mechanism
carries its own default).

#### Required changes

| Surface | Before | After |
| ------- | ------ | ----- |
| `[dirsql].hook-timeout` config key | `hook-timeout = 300` bounded every `on-file` run (default 30s) | Delete the key (a config carrying it fails to load, naming the replacement); to bound a hook, wrap its command: `on-file = "timeout 300 my-extractor {path}"`. On Windows, `timeout` is a sleep, not a wrapper — bound the work inside the command or accept unbounded runs |
| `[[dirsql.function]]` entries relying on `hook-timeout` as their per-call default | `hook-timeout = N` was the default bound for function calls with no `timeout` of their own | Declare the bound on the function entry itself (`timeout = "300s"`); with no `timeout`, the mechanism's 30-second default applies |
| Rust `command::run_command` | `run_command(command, placeholders, cwd, timeout, stdin_payload)`; an overrun returned `CommandError::Timeout` | `run_command(command, placeholders, cwd, stdin_payload)` — runs unbounded; `CommandError::Timeout` and `DEFAULT_COMMAND_TIMEOUT` no longer exist. A `timeout(1)` wrapper's kill surfaces as `CommandError::NonZeroExit` |
| Rust `functions` module | No public default-timeout constant | `functions::DEFAULT_FUNCTION_TIMEOUT` (`Duration::from_secs(30)`) is public |

#### Deprecations removed

_None._ (The key was removed outright, with no deprecation period.)

#### Behavior changes without code changes

- A slow `on-file` hook that previously hit the 30-second default and had
  its file skipped now runs to completion (and its rows land). If you were
  relying on the implicit bound, wrap the command in `timeout(1)`.
- A `.dirsql.toml` declaring `hook-timeout`: previously loaded (bounding
  hook runs); now the server degrades and `dirsql query` exits non-zero,
  with an error naming the `timeout(1)` replacement.
- A `[[dirsql.function]]` entry with no `timeout` behaves identically
  (30-second default) unless its config also declared `hook-timeout`, which
  previously served as the default bound for its calls.

#### Verification

```bash
printf '[dirsql]\nhook-timeout = 30\n' > /tmp/timed.toml
dirsql query "SELECT 1 AS one" -c /tmp/timed.toml
# expected: non-zero exit; stderr names 'hook-timeout' and the timeout(1) idiom

dirsql query "SELECT 1 AS one"
# expected: [{"one":1}]
```
