**Fixed**

`db.query()` and `db.queryOrdered()` now read what is on disk when they are called. Before handing the SQL to the threadpool, the call walks each named table's glob on the JavaScript thread, runs `onFile` on every file whose stat moved or that changed within the last two seconds, and drops the rows of files that are gone, so a write made just before a query is never missed. The row events those re-reads produce reach `watch()` streams once each, including a change read before the first `watch()` call.
