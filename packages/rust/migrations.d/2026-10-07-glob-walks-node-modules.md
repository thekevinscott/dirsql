### Globs walk node_modules

#### Summary

Globs no longer skip `node_modules`; nothing else changes.

#### Required changes

_None._

#### Deprecations removed

_None._

#### Behavior changes without code changes

- **Globs walk `node_modules`, like bash.** A path-table or config glob such as `./**/*.js` now returns files under `node_modules`. A `.gitignore` that lists it still hides it.

#### Verification

```bash
mkdir -p /tmp/nm/node_modules/p && touch /tmp/nm/node_modules/p/a.js && cd /tmp/nm && dirsql query "SELECT path FROM './**/*.js'"   # node_modules/p/a.js
```
