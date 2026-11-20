**Changed**

Once one query has had to wait for another, queries run concurrently: the index is copied once into a WAL-mode file in SQLite's temp directory, and each query then reads a snapshot on a reader connection of its own, so a cheap query no longer waits behind a slow one and a watch batch lands while a slow query runs without that query seeing it. Queries still wait for a batch already being applied. Queries that name a batched function run one at a time.
