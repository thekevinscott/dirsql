**Fixed**

A query on a long-lived `DirSQL` now reads what is on disk when it starts (core change, surfaced through the Python SDK). Before reading, it walks each named table's glob, re-runs `on_file` on every file whose stat moved or that changed within the last two seconds, and drops the rows of files that are gone, so a write made just before a query is never missed. The row events those re-reads produce reach `watch()` streams once each, including a change read before the first `watch()` call.
