**Added** the `dirsql-plugin-embeddings worker` subcommand: a persistent
stdin/stdout process serving `embed()` requests as newline-delimited JSON
(`{"call": [value, model_id?]}` → `{"ok": [floats...]}` / `{"err": "message"}`).
Values are SQL TEXT (JSON strings) or BLOB (`{"$bytes": "<base64>"}`, decoded
as utf-8); embeddings come from model2vec (default
`minishlab/potion-retrieval-32M`, loaded lazily on the first request, one load
per process; an optional second argument overrides the model id). Vectors are
cached on disk via cachetta at `~/.cache/dirsql/embeddings/` (respects
`XDG_CACHE_HOME`; safe to wipe — the only cost is re-embedding), keyed by the
SHA-256 of the value bytes plus the model identifier. Progress is reported on
stderr only when stderr is a TTY. The packaged `dirsql.toml` now declares the
`embed` SQL function via `[[dirsql.function]]` (name `embed`, arities 1–2,
deterministic, 600s per-call timeout) alongside the existing sqlite-vec
`[[dirsql.extension]]` entry.
