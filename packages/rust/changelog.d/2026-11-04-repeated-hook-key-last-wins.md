**Fixed** A key repeated within one hook output row now keeps its last value
everywhere. `--on-file` (and an SDK `path_table_parser`) used to keep the
first value while a `[[table]]` `on-file` kept the last, so the same command
gave two answers.
