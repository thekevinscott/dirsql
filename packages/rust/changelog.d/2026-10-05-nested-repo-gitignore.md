**Fixed** Path-table scans stop applying an outer repository's `.gitignore`
rules inside a nested repository. A nested repo now starts a fresh ignore
boundary, matching Git; `'./**'` and narrower globs return the same files.
