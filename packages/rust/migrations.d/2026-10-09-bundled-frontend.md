### Bundled server frontend

#### Summary

The Rust CLI and Python and TypeScript launchers can serve a bundled SQL explorer shell with `dirsql server --frontend`. Existing invocations keep their behavior. Rust callers constructing `ServerConfig` with a struct literal must add the new field.

#### Required changes

| Before | After |
|---|---|
| `ServerConfig { host, port, query_timeout, cors_origin }` | `ServerConfig { host, port, query_timeout, cors_origin, frontend: false }` for direct struct construction |

Callers using `ServerConfig::bind`, `ephemeral`, or `default` need no changes.

#### Deprecations removed

_None._

#### Behavior changes without code changes

_None._

#### Verification

Run `dirsql server --frontend --port 0` and open the printed URL. The page shows “dirsql SQL explorer” and the server state.
