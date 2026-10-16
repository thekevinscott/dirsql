**Changed**

- **Rows parsed from `on-file` output share one string per key instead of carrying their own copy.** A config table over large JSONL output, such as millions of log events, loads faster. `infer::JsonRow` keys are now `Arc<str>`. Query results are unchanged.
