**Changed**

- `resolve_extension_path` now plans through the core
  (`_dirsql.plan_extension_path`) instead of re-implementing the ordered probe,
  so a programmatic `extensions=[{path}]` entry is classified by the same code
  as a config entry. `importlib` package lookup is unchanged, and so is the
  resolved path. The internal `_dirsql.is_bare_name` binding export is gone,
  replaced by `plan_extension_path`. No change to the published API. (#1122)
