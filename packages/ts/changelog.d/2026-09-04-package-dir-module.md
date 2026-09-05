**Changed**

- **Extension package-directory lookup moved to its own module.** `packageDir` (and the `PackageResolver` seam it consumes) now lives in `src/package-dir.ts`; `resolveExtensionPath`'s package probe imports it. Internal reorganisation with no change to the published API or extension-resolution behavior. (#1042)
