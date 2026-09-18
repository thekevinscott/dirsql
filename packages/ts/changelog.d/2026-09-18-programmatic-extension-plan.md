**Changed**

- `resolveExtensionPath` now plans through the core (the addon's
  `planExtensionPath`) instead of re-implementing the ordered probe, so a
  programmatic `extensions: [{ path }]` entry is classified by the same code as
  a config entry. `require.resolve` package lookup is unchanged, and so is the
  resolved path. The internal `isBareName` addon export is gone, replaced by
  `planExtensionPath`. No change to the published API. (#1122)
