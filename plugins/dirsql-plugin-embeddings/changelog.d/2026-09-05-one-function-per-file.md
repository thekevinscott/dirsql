**Changed**

Split `search/sql.py`, `search/run.py`, and `embedding/model.py` into one
module per non-trivial function, so the tree is clean at
`one-function-per-file`'s strictest threshold. Every symbol keeps its behavior;
`search.run._search` is now `search.search_rows.search_rows`, and the SQL
builders moved to modules named for them.
