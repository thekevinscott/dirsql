**Changed**

- **Extension resolution now plans in the Rust core.** `resolveConfigsExtensionSpecs` reads each config off disk and hands `{ path, contents }` pairs to the addon's `planConfigExtensions`; `resolvePackage` hands the located package directory and its files to `selectLoadable`; `resolveExtensionPath` classifies through `isBareName`. TypeScript keeps only the two host-specific halves — reading the files, and `require.resolve`. Six internal modules go away (`is-bare-name`, `has-bare-name`, `load-extension-entries`, `resolve-entries`, `platform-suffixes`, `resolve-config-extension-specs`) against two added (`read-config`, `plan-entry-path`), and with them the `smol-toml` dependency and the lazy-load guard it needed (#720). No change to the published API. (#1121)

**Fixed**

- The error raised when a config's `[[dirsql.extension]]` names an installed package with no loadable file now writes the searched suffixes as glob patterns — `no loadable extension file (*.so / *.node) found in package 'x' (searched /dir)` — matching the Python SDK. (#1121)
