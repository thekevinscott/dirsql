**Changed**

- **Two extension-resolution helpers moved to their own modules.** The package-directory glob (`resolvePackage`, with the platform-suffix table it consumes) now lives in `src/resolve-package.ts`, and the TOML `[[dirsql.extension]]` reader (`loadExtensionEntries`, with the `Toml` alias) in `src/load-extension-entries.ts`. `resolveExtensionPath`, `isBareName`, `resolveConfigExtensionSpecs` and `resolveConfigsExtensionSpecs` stay where they were. Internal reorganisation with no change to the published API or extension-resolution behavior. (#1055)
