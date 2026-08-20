**Added** — `dirsql::cli::run_cli_with_version(argv, version)`: the CLI entry
point with the reported version supplied by the caller, for embedders that ship
this crate inside an artifact published on its own version line (#958).
`run_cli` is unchanged.
