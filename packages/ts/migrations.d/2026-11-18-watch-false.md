### TypeScript SDK: `watch: false` opts out of the watcher

**Summary**

`new DirSQL({ watch: false })` builds from one walk and starts no watcher; `watch()`, `startWatcher()` and `pollEvents()` throw. Additive: the option defaults to watching, so nothing breaks.

**Required changes**

_None._

**Deprecations removed**

_None._

**Behavior changes without code changes**

_None._

**Verification**

```bash
node -e "const { DirSQL } = require('dirsql'); try { new DirSQL({ watch: false }).watch() } catch (e) { console.log(e.message) }"
# watching is off: this instance was built with watch disabled
```
