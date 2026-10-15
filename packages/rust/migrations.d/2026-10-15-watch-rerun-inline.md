### Faster watch reruns

#### Summary

A per-table `on-file` rerun over files that fit one invocation runs the command directly, without extra threads or a CPU count. Events and their order are unchanged; nothing breaks.

#### Required changes

_None._

#### Deprecations removed

_None._

#### Behavior changes without code changes

_None._

#### Verification

```bash
dirsql server
```

Writing to an indexed file emits the same `/events` row as before, sooner.
