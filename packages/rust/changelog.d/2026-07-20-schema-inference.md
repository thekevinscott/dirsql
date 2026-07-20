**Added**

Schema inference from row-object output. A new `infer` module derives a SQLite
column list from a parser's JSON rows — the union of their keys in first-seen
order, typed by the values seen under each (string → `TEXT`, integer →
`INTEGER`, float → `REAL`, bool → `INTEGER`, nested object/array → its JSON
text; null and missing carry no type, and any disagreement falls back to
`TEXT`). A new `dirsql_parsed('<root>', '<glob>', '<parser>')` virtual table
uses it: the parser runs over every matched file at registration, the inferred
schema is declared, and those rows are served — a table without DDL.

Core-only; the CLI flag that exposes it follows separately.
