**Changed**

- **One-shot `dirsql query` no longer enforces the 30-second per-query timeout.** The query runs to completion; bound it externally when you want a cap (`timeout 60 dirsql query "<sql>"`). Server mode is untouched: `POST /query` still enforces `ServerConfig.query_timeout` (default 30s) and returns `408`. Rust API: `cli::execute::execute_query` now takes `Option<Duration>` (`None` = unbounded). (#819)
