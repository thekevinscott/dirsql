### Missing-loadable error wording is now glob-shaped (#1121)

#### Summary

Extension resolution moved onto the Rust core's plan contract
(`dirsql::extension_resolution`), so the "no loadable extension file" error is
now rendered by the core rather than by the TypeScript port of it. The core
writes each searched suffix as a glob, which changes the message text.

#### Required changes

_None._ No exported symbol, argument, or config key changed.

#### Deprecations removed

_None._

#### Behavior changes without code changes

- A config `[[dirsql.extension]]` naming an installed package that ships no
  loadable file rejects with `no loadable extension file (*.so / *.node) found
  in package 'x' (searched /dir)`. It previously read `(.so / .node)`. Code
  matching that string exactly must be updated; the Python SDK already emitted
  the glob form, so the two now agree.
- `smol-toml` is no longer a runtime dependency. Nothing imported it from
  outside the package, but a consumer relying on it being hoisted into
  `node_modules` by `dirsql` must depend on it directly.

#### Verification

```bash
mkdir -p node_modules/empty-ext
echo '{"name":"empty-ext","version":"1.0.0"}' > node_modules/empty-ext/package.json
printf '[[dirsql.extension]]\npath = "empty-ext"\n' > .dirsql.toml
npx dirsql query "SELECT 1" --config .dirsql.toml
# -> no loadable extension file (*.so / *.node) found in package 'empty-ext' (searched …/node_modules/empty-ext)
```
