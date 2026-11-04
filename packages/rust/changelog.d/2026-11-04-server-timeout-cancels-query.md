**Fixed** A `dirsql server` query answered `408 Request Timeout` now stops
running. It used to keep running in the background while holding the index,
so every later query waited on it and timed out too; a query that never ended
wedged the server until restart. The next query is now served at once.
