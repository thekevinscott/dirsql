### A wildcard under a gitignored literal prefix is hidden

#### Summary

A path-table such as `./dist/*.js` no longer returns files when `dist/` is gitignored, matching how `./*.log` is hidden. Nothing in the API changes.

#### Required changes

_None._

#### Deprecations removed

_None._

#### Behavior changes without code changes

- `./dist/*.js`, `./dist/**/*.js` and `./dist/sub/*.js` return nothing when `dist/` is gitignored inside a git repo. Name the file (`./dist/bundle.js`) or the directory (`./dist`) to list it, or pass `--no-ignore`.

#### Verification

```bash
dirsql query "SELECT path FROM './dist/*.js'"
```
