**Added**

- **`dirsql -c`/`--config` is now repeatable.** Pass several config files (`dirsql -c a.toml -c b.toml`, also on `dirsql query`); they load and merge in argv order — `[[table]]`, `ignore`, and `[[dirsql.extension]]` entries accumulate, and `pre-query` / `post-query` hooks chain FIFO. Each config's hooks run from its own directory under its own `[dirsql].hook-timeout`; a duplicate table name across configs errors. A single `-c` (or none, defaulting to `./.dirsql.toml`) is unchanged. (#547)
