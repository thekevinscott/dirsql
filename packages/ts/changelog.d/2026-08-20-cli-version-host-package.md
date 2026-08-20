**Fixed** — `dirsql --version` reports the installed package's version instead
of a frozen `0.2.7`: it read the embedded core crate's literal, which only the
crates.io release lane rewrites (#958). The launcher now passes its own
`package.json` version to the addon.
