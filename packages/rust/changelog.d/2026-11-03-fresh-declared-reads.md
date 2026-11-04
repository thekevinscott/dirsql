**Fixed**

A query on a long-lived `DirSQL` now reads what is on disk when it starts. Before reading, it walks each named table's glob, re-parses every file whose stat moved or that changed within the last two seconds, and drops the rows of files that are gone, so a write made just before a query is never missed. The row events those re-reads produce reach `watch()` streams and the next `poll_events()`, once each. A change a query reads before any watcher starts reaches the first `watch()` stream or poll. The one-shot `dirsql query` reads the index as built, unchanged.
