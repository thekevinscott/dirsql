### A named gitignored directory lists

#### Summary

A path-table `./dist/` and a config glob `dist` list the files directly inside a gitignored `dist/`, as `./dist` already did. Nothing breaks.

#### Required changes

_None._

#### Deprecations removed

_None._

#### Behavior changes without code changes

- With `dist/` gitignored, `./dist/` and a config glob naming `dist` or `dist/` return the files directly inside it instead of nothing. A wildcard such as `dist/*` is still hidden.

#### Verification

```bash
dirsql query "SELECT path FROM './dist/'"
```
