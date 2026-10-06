### Watcher startup failures name the inotify limit; `/events` returns 503 (#1398)

#### Summary

On Linux, the watcher error for an exhausted inotify limit now adds the limit's name and the sysctl that raises it. `dirsql server` answers `GET /events` with `503` when its watcher failed to start, where it used to open a stream that never delivered events. Nothing breaks.

#### Required changes

_None._

#### Deprecations removed

_None._

#### Behavior changes without code changes

- `watcher error: Too many open files (os error 24)` gains `; the inotify instance limit is likely exhausted: raise it with `sudo sysctl fs.inotify.max_user_instances=<higher value>``. `OS file watch limit reached.` gains the same hint naming `fs.inotify.max_user_watches`.
- `GET /events` returns `503` with `{"error": "filesystem watcher failed to start: <reason>"}` instead of `200`, `event: ready` and then no events.

#### Verification

```bash
cd /tmp
dirsql server &   # on a host whose inotify instances are exhausted
curl -i http://localhost:7117/events   # HTTP/1.1 503, {"error":"filesystem watcher failed to start: watcher error: ... fs.inotify.max_user_instances ..."}
```
