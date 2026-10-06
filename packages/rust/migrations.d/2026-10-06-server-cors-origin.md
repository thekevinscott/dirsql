### Opt-in CORS for `dirsql server` (#1399)

#### Summary

`dirsql server` gains `--cors-origin <origin>`, and `ServerConfig` gains a `cors_origin` field with a `with_cors_origin` builder. Without the flag the server behaves as before. Code that builds `ServerConfig` through `default()`, `ephemeral()` or `bind()` is unaffected.

#### Required changes

| Before | After |
|---|---|
| `ServerConfig { host, port, query_timeout }` | `ServerConfig { host, port, query_timeout, cors_origin: None }` |

#### Deprecations removed

_None._

#### Behavior changes without code changes

_None._

#### Verification

```bash
dirsql server --cors-origin http://localhost:3202 &
curl -si -X OPTIONS localhost:7117/query -H 'origin: http://localhost:3202' -H 'access-control-request-method: POST' -H 'access-control-request-headers: content-type' | grep -i access-control-allow-origin   # access-control-allow-origin: http://localhost:3202
```
