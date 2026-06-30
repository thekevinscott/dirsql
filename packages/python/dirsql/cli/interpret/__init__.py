"""Empty package shell — the native-config ``interpret`` helper was removed.

The ``dirsql interpret`` subcommand and its NDJSON ``extract`` loop (``run``,
``load_app``, ``dispatch_extract``, ``write_message``) were removed in #321
(#323). The CLI now accepts only ``.dirsql.toml``; to run user-defined
``extract`` callbacks, use the programmatic SDK (``DirSQL(...)`` with
in-process closures).

This ``__init__.py`` carries no logic and no re-exports. It remains only
because the colocated-test tooling cannot yet express *deleting* an exempt
package barrel (the co-change check flags a deleted source that has no
co-deleted colocated test, and a retained exempt for a deleted path is
rejected as stale). The directory is removed once that is resolved.
"""
