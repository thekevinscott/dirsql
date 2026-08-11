**Removed**

- **The `pre-query` and `post-query` hooks are removed.** The `[dirsql].pre-query` / `[dirsql].post-query` config keys, their command-execution plumbing, and the `cli::PreQuery` / `cli::PostQuery` types (with `ServerConfig::with_pre_query` / `with_post_query`) are gone; a config carrying either key now fails with the standard unknown-key error naming it. `POST /query` always parses its body as `{"sql": …}` and always returns the bare row array. `hook-timeout`, `on-file`, and `[[table]]` are untouched. (#803)
