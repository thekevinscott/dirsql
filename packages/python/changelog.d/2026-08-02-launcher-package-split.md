**Changed** — the `dirsql` console script now lives in its own top-level
`dirsql_cli` package, so launching the CLI no longer imports the SDK. Running
`dirsql` previously executed `dirsql/__init__.py` first, dlopening the
`_dirsql` extension and importing asyncio, then discarded all of it moments
later at `os.execv`. After a `dirsql` invocation none of `dirsql`,
`dirsql._dirsql` or `asyncio` is imported.

The two resolver modules the launcher shares with the SDK moved to a top-level
`_dirsql_shared` package for the same reason.

No change to the public API or to any CLI behavior: `from dirsql import DirSQL,
Table, RowEvent, __version__` is unchanged, and the `dirsql` command's output
and exit codes are identical. Measured effect on `dirsql --version` is modest
-- ~73 ms to ~67 ms warm, unchanged cold -- since the launcher's remaining cost
is interpreter startup plus the `importlib.metadata` plugin scan.
