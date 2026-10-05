### npm: `dirsql --version` reports the published version

#### Summary

The npm CLI's `--version` printed `dirsql 0.2.7` whatever version was
installed. It now prints the version of the installed `dirsql` package, as
the PyPI build already did. Nothing breaks.

#### Required changes

_None._

#### Deprecations removed

_None._

#### Behavior changes without code changes

- `npx dirsql --version` prints `dirsql <installed version>` instead of
  `dirsql 0.2.7`.

#### Verification

```sh
npx dirsql@latest --version
# expected: dirsql <the version `npm view dirsql version` prints>
```
