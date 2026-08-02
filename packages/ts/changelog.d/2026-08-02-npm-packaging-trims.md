**Changed** — the published tarball no longer ships source maps, and the CLI
launcher no longer loads the TOML parser unless a TOML config is actually
present.

Both `.js.map` and `.d.ts.map` pointed at `../src/*.ts`, which `files` does not
ship, so in the published package they resolved to nothing. `tsconfig.build.json`
now turns both off; the base `tsconfig.json` keeps them on for local debugging.
The tarball drops from 64,646 to 57,903 bytes and unpacked `dist/` from 67,593
to 41,882 bytes.

`withResolvedExtensions` is now async and imports the shared config-extension
resolver -- and through it `smol-toml` -- only after confirming the config file
exists. Running `dirsql` with no TOML config on disk no longer loads the parser
at all.

No behavior change: argv handling, resolved `--extension` flags, error messages
and exit codes are unchanged.
