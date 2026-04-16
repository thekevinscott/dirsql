# Tests Audit (bead: dirsql-9ng)

Audit mapping between documented features and tests. Produced 2026-04-14 as part
of enforcing the "docs as spec" rule in `AGENTS.md` (every documented feature
has a test; every test traces back to a documented feature).

Scope of docs surveyed:
- `docs/index.md`, `docs/getting-started.md`
- `docs/guide/tables.md`, `docs/guide/querying.md`, `docs/guide/async.md`,
  `docs/guide/watching.md`, `docs/guide/config.md`
- `docs/api/index.md`
- `packages/python/README.md`, `packages/ts/README.md`

Scope of tests surveyed:
- `packages/python/tests/integration/*.py`
- `packages/ts/test/*.test.ts`
- `packages/rust/tests/*.rs` (plus inline `#[cfg(test)]` in `packages/core/src/*.rs`
  where relevant)

Legend: "P" = Python, "R" = Rust, "T" = TypeScript.

---

## 1. Documented -> tested

| Doc feature | Doc location | Test(s) |
|---|---|---|
| Quick start blog example (index, join) | `docs/getting-started.md` "Quick start" | P: `test_docs_examples.py::describe_getting_started::*`; R: `docs_examples.rs::it_matches_getting_started_*` |
| Hero example: query with WHERE size > 1000 | `docs/index.md` | Covered indirectly by `describe_querying_guide::it_matches_querying_guide_where_filter` (P/R) |
| `Table(ddl, glob, extract)` constructor | `docs/guide/tables.md` "Table constructor"; `docs/api/index.md` Table | P: `test_dirsql.py::describe_DirSQL::describe_init`; R: `sdk.rs::it_indexes_and_queries_rows`; T: `index.test.ts "creates an instance and queries data"` |
| DDL parsed for table name (SQLite types/constraints) | `docs/guide/tables.md` "ddl" | P/R: `*::it_matches_tables_guide_typed_columns`, `*::it_matches_tables_guide_constraints` |
| `glob` (standard Unix globbing, `**`) | `docs/guide/tables.md` "glob" | P/R: tables guide tests; T: `index.test.ts "supports glob patterns"` |
| `extract(path, content) -> list[dict]` contract | `docs/guide/tables.md` "extract"; `docs/api/index.md` Table | P: `test_dirsql.py::describe_extract_receives_path_and_content`; R: `sdk.rs::it_indexes_and_queries_rows` (derives id from path); T: `index.test.ts` parses content |
| Return `[]` from extract to skip | `docs/guide/tables.md` "extract" (conditional skip example) | P/R: `*::it_matches_tables_guide_skip_*` |
| Multiple tables | `docs/guide/tables.md` "Multiple tables" | P/R: `*::it_matches_tables_guide_multiple_tables`; T: `index.test.ts "supports multiple tables"` |
| `ignore` parameter | `docs/guide/tables.md` "Ignore patterns"; `docs/api/index.md` | P/R: `*::it_matches_tables_guide_ignore_patterns`; T: `index.test.ts "supports ignore patterns"` |
| Value-type map: str, int, float, bool, None | `docs/guide/tables.md` "Supported value types" | P: `test_docs_examples.py::it_matches_tables_guide_value_types` |
| Value-type map: `bytes` -> `BLOB` | `docs/guide/tables.md` "Supported value types" | **Added** in this PR: P `test_docs_gaps.py::describe_tables_guide_bytes_to_blob::it_maps_python_bytes_to_sqlite_blob`. Rust/TS gap noted in section 3. |
| `query(sql)` returns list of dicts | `docs/guide/querying.md` "Basic queries"/"Return format"; `docs/api/index.md` | P/R: `*::it_matches_querying_guide_select_all`, `*::it_matches_querying_guide_return_format`; T: `index.test.ts "creates an instance and queries data"` |
| WHERE, aggregation (COUNT, GROUP BY), JOINs | `docs/guide/querying.md` "Basic queries" | P/R: `*::it_matches_querying_guide_where_filter`, `*::it_matches_querying_guide_aggregation`, `*::it_matches_querying_guide_join`; T: `index.test.ts "handles SQL queries with WHERE clauses"` |
| Internal columns (`_dirsql_file_path`, `_dirsql_row_index`) excluded | `docs/guide/querying.md` "Internal columns"; `docs/api/index.md` | P/R: `*::it_matches_querying_guide_internal_columns_excluded` |
| Invalid SQL raises | `docs/guide/querying.md` "Error handling" | P/R: `*::it_matches_querying_guide_error_handling`; T: `index.test.ts "throws on invalid SQL"` |
| Empty result set | `docs/guide/querying.md` "Empty results" | P/R: `*::it_matches_querying_guide_empty_results` |
| Async constructor is non-blocking (Py) | `docs/guide/async.md` "Constructor" | P: `test_async_dirsql.py::describe_init::it_creates_instance_synchronously` |
| `await db.ready()` + idempotency + re-raise | `docs/guide/async.md` "ready"; `docs/api/index.md` | P: `describe_init::it_indexes_files_after_ready`, `it_allows_multiple_ready_calls`, `it_raises_on_extract_error_during_ready`; R: `async_sdk.rs::it_allows_multiple_ready_calls`, `it_constructs_without_blocking` |
| `await db.query(...)` async | `docs/guide/async.md` "query"; `docs/api/index.md` | P: `describe_query::*`; R: `async_sdk.rs::it_queries_asynchronously` |
| `watch()` insert/update/delete/error events | `docs/guide/watching.md` event types; `docs/api/index.md` | P: `test_async_dirsql.py::describe_watch::*`, `test_docs_examples.py::describe_watching_guide_*_event`; R: `sdk.rs::it_streams_watch_*_events`, `docs_examples.rs::it_matches_watching_guide_*_event` |
| `watch()` async iterable yields events (Py/Rust) | `docs/guide/watching.md`; `docs/api/index.md` | P: `test_async_dirsql.py::describe_watch::*`; R: `async_sdk.rs::it_streams_watch_events` |
| `RowEvent.action/table/row/old_row/error/file_path` | `docs/guide/watching.md` event payloads; `docs/api/index.md` RowEvent | P: `test_docs_examples.py::describe_watching_guide_*` (asserts action/table/row); R: `sdk.rs`/`docs_examples.rs`; `file_path` covered via `test_docs_examples.py::it_matches_watching_guide_insert_event` (asserts non-None) |
| `RowEvent.file_path` is **relative** to root | `docs/guide/watching.md` (all examples show relative paths) | **Added**: P `test_docs_gaps.py::describe_watching_guide_positional_identity_gap::it_sets_file_path_as_relative_path_on_events` |
| Diffing: identical content emits nothing; appends emit inserts; full replace on heavy edits | `docs/guide/watching.md` "How diffing works" | Core unit tests in `packages/core/src/differ.rs` (`no_events_when_content_identical`, `insert_events_for_appended_lines`, `full_replace_when_more_than_half_changed`) |
| Diffing: shrinking file drops dropped rows (end state) | `docs/guide/watching.md` "How diffing works" | **Added**: P `test_docs_gaps.py::describe_watching_guide_positional_identity_gap::it_emits_delete_for_shrinking_file_positionally`. See divergence in section 4: the *mechanism* described (positional identity) is not what the implementation does (full replace on shrink). |
| `from_config(path)` basic indexing | `docs/guide/config.md` "Basic Example"; `docs/api/index.md` | P: `test_from_config.py::describe_basic`; R: `from_config.rs::from_config_indexes_csv_files` |
| Format inference: `.json` | `docs/guide/config.md` "Supported Formats" | P: `test_from_config.py::describe_basic::it_loads_json_files_via_config`; R: `from_config.rs::from_config_with_json_and_each` |
| Format inference: `.jsonl` | `docs/guide/config.md` "Supported Formats" | P: `test_from_config.py::describe_basic::it_loads_jsonl_files_via_config` |
| Format inference: `.csv` | `docs/guide/config.md` "Supported Formats" | P: `describe_basic::it_loads_csv_files_via_config`; R: `from_config.rs::from_config_indexes_csv_files` |
| Format inference: `.tsv` | `docs/guide/config.md` "Supported Formats" | **Added**: P `test_docs_gaps.py::describe_from_config_formats_gap::it_loads_tsv_files_via_config` |
| Format inference: `.ndjson` | `docs/guide/config.md` "Supported Formats" | **Added**: P `test_docs_gaps.py::describe_from_config_formats_gap::it_loads_ndjson_files_via_config` |
| Format inference: `.toml` | `docs/guide/config.md` "Supported Formats" | **Added**: P `test_docs_gaps.py::describe_from_config_formats_gap::it_loads_toml_files_via_config` |
| Format inference: `.yaml` / `.yml` | `docs/guide/config.md` "Supported Formats" | **Added**: P `test_docs_gaps.py::describe_from_config_formats_gap::it_loads_yaml_files_via_config[yaml\|yml]` |
| Format inference: `.md` with frontmatter + body | `docs/guide/config.md` "Supported Formats" | **Added**: P `test_docs_gaps.py::describe_from_config_formats_gap::it_loads_markdown_with_frontmatter_via_config` |
| Explicit `format = "..."` override | `docs/guide/config.md` "Supported Formats" (implied by override), tested in practice | P: `test_from_config.py::describe_explicit_format`; R: `from_config.rs::from_config_with_explicit_format` |
| Path captures `{name}` in globs | `docs/guide/config.md` "Path Captures" | P: `test_from_config.py::describe_path_captures`; R: `from_config.rs::from_config_with_path_captures` |
| `each` for nested JSON navigation | `docs/guide/config.md` "Nested Data" | P: `test_from_config.py::describe_each`; R: `from_config.rs::from_config_with_json_and_each` |
| `[table.columns]` column mapping | `docs/guide/config.md` "Column Mapping" | P: `test_from_config.py::describe_column_mapping`; R: `from_config.rs::from_config_with_column_mapping` |
| `[dirsql].ignore` in config | `docs/guide/config.md` "Ignore Patterns" | P: `test_from_config.py::describe_ignore`; R: `from_config.rs::from_config_honors_ignore_patterns` |
| Relaxed schema (default): extra keys dropped, missing keys NULL | `docs/guide/config.md` "Strict Mode" (implicit via default) | P: `test_dirsql.py::describe_relaxed_schema`; R: `sdk.rs::it_ignores_extra_keys_by_default`, `it_fills_missing_keys_with_null` |
| `strict = true` in `.dirsql.toml` | `docs/guide/config.md` "Strict Mode" | **Added**: P `test_docs_gaps.py::describe_from_config_strict_mode_gap::it_raises_on_extra_keys_when_strict_true`, `it_allows_exact_match_when_strict_true`. Strict at the Python `Table(strict=True)` layer is already covered in `test_dirsql.py::describe_relaxed_schema::it_raises_on_*_in_strict_mode`. Rust `Table::strict` covered in `sdk.rs`. |
| `from_config` error cases (missing file, invalid TOML, missing DDL, unsupported ext) | `docs/api/index.md` / `docs/guide/config.md` behavior | P: `test_from_config.py::describe_error_handling`; R: `from_config.rs::from_config_missing_config_file_returns_error`, `from_config_no_format_and_unknown_extension_returns_error` |
| `from_config` async (Py) / sync (Rust) behaviour | `docs/api/index.md` "from_config" | P: `test_from_config.py::describe_query_after_config`; R: `from_config.rs::async_from_config_works` |
| README quick-start examples | `packages/python/README.md`, `packages/ts/README.md` | Python quick-start is structurally the same as `docs/getting-started.md` and covered by `test_docs_examples.py`. TS README is covered by `index.test.ts`. |

---

## 2. Documented but not tested -> gaps filled in this PR

All gaps identified in section 1 have been filled. Summary of tests added by this PR:

| Gap | New test |
|---|---|
| `bytes -> BLOB` (docs/guide/tables.md) | `packages/python/tests/integration/test_docs_gaps.py::describe_tables_guide_bytes_to_blob::it_maps_python_bytes_to_sqlite_blob` |
| `.tsv` via `from_config` | `test_docs_gaps.py::describe_from_config_formats_gap::it_loads_tsv_files_via_config` |
| `.ndjson` via `from_config` | same describe, `it_loads_ndjson_files_via_config` |
| `.toml` via `from_config` | same describe, `it_loads_toml_files_via_config` |
| `.yaml` / `.yml` via `from_config` | same describe, `it_loads_yaml_files_via_config[yaml\|yml]` |
| `.md` frontmatter via `from_config` | same describe, `it_loads_markdown_with_frontmatter_via_config` |
| `strict = true` in `.dirsql.toml` | `describe_from_config_strict_mode_gap::it_raises_on_extra_keys_when_strict_true` + `it_allows_exact_match_when_strict_true` |
| Watching: shrinking-file row drop end-state | `describe_watching_guide_positional_identity_gap::it_emits_delete_for_shrinking_file_positionally` |
| `RowEvent.file_path` is a relative path | `describe_watching_guide_positional_identity_gap::it_sets_file_path_as_relative_path_on_events` |

All 11 new tests pass. Full Python integration suite (85 tests), Rust test suite
(all `cargo test` binaries + doc-tests), and TypeScript Vitest suite (8 tests)
are green.

---

## 3. Tested but not documented -> recommendations (surfaced, not changed)

Per bead scope: surface these, do NOT delete or alter docs. These are candidates
for either promotion into the docs or removal.

- `test_dirsql.py::describe_query::it_handles_integer_values` — trivially
  covered by the documented value-type map. **Recommendation:** keep; it is a
  direct value-of-documented-behavior test.
- `test_dirsql.py::describe_error_handling::it_raises_on_invalid_ddl` —
  behavior (invalid DDL raises) is implicit in the docs ("`dirsql` executes
  this DDL directly against the in-memory database") but not stated
  explicitly. **Recommendation:** promote to docs (add a sentence in
  `docs/guide/tables.md` "ddl" noting invalid DDL raises during scan/ready).
  Same applies to `docs_examples.rs` / TS `"throws on invalid DDL"`.
- `test_dirsql.py::describe_error_handling::it_handles_empty_directory` —
  behavior is reasonable and implied but not explicitly documented.
  **Recommendation:** document briefly in `docs/getting-started.md` or
  `docs/guide/querying.md` "Empty results".
- `test_dirsql.py::describe_error_handling::it_handles_extract_returning_empty_list`
  — duplicates the "return `[]` to skip" doc example and is useful as a
  stand-alone assertion. **Recommendation:** keep.
- `test_dirsql.py::describe_extract_receives_path_and_content::it_passes_relative_path_and_string_content`
  — confirms `path` is relative and `content` is `str`. The docs describe
  `path` as "the file path relative to the root" so this is covered; worth
  keeping as a direct assertion. **Recommendation:** keep; mirror in Rust/TS
  for parity.
- `test_dirsql.py::describe_relaxed_schema` (4 tests) — strict-mode at the
  `Table(strict=True)` Python constructor level. The docs (`docs/guide/config.md`
  "Strict Mode") document `strict = true` in the TOML config but do not document
  `Table(strict=True)` as a public Python SDK flag. **Recommendation:** either
  document `Table(strict=True)` in `docs/guide/tables.md`/`docs/api/index.md`
  or drop the flag from the public surface. This is an API that exists and is
  tested but is not specified.
- `test_async_dirsql.py::describe_watch::it_updates_db_on_file_changes` —
  validates DB state after filesystem events; the invariant is implicit in
  `docs/guide/watching.md` "Updates the in-memory database to reflect the new
  state". **Recommendation:** keep.
- `packages/core/src/differ.rs` inline tests cover edge cases
  (`no_full_replace_when_exactly_half_changed`,
  `full_replace_deletes_before_inserts`) that are finer-grained than the docs.
  **Recommendation:** keep as internal unit tests; no doc action needed.
- `packages/core/src/db.rs` inline tests exercising `Value::Blob` — correspond
  to documented `bytes -> BLOB` mapping. **Recommendation:** keep.
- `packages/ts/test/index.test.ts "handles empty directories gracefully"` /
  `"throws on invalid DDL"` — mirror Python tests above; same recommendations.

No tests were found that had no plausible link back to a documented feature, so
no deletions are recommended.

---

## 4. Doc / Impl Divergence

These are cases where the docs describe behavior that the implementation does
not actually provide. Per this bead's scope, they are surfaced for review and
**not fixed** here.

### 4.1 TypeScript SDK: docs describe async API; implementation is synchronous

- `docs/api/index.md` and `docs/guide/async.md` show, for TypeScript:
  - `await db.ready` as "awaitable property"
  - `await db.query(sql: string): Promise<Record<string, unknown>[]>`
  - `db.watch(): AsyncIterable<RowEvent>` consumed via `for await`
- The actual `packages/ts/ts/index.ts` public surface is:
  - No `ready()` / `ready` at all (scanning is synchronous in the constructor).
  - `query(sql): Record<string, unknown>[]` — synchronous, not a `Promise`.
  - `startWatcher()` + `pollEvents(timeoutMs)` — no `AsyncIterable<RowEvent>`,
    no `watch()` method.
- Knock-on effect: `packages/ts/README.md` shows the sync usage
  (`db.query("SELECT ...")` without `await`), which is correct for the
  implementation but contradicts the main docs site.
- **Recommendation:** either (a) land the async TS API described in the docs
  (probably an `AsyncDirSQL` wrapper that awaits `query` and exposes
  `watch()` as an `AsyncIterable` over `pollEvents`), or (b) rewrite the TS
  blocks in `docs/guide/async.md`, `docs/guide/watching.md`, and
  `docs/api/index.md` to match the current sync surface. This is a parity
  issue tracked separately from this bead.

### 4.2 Watching: "positional row identity" description does not match implementation

- `docs/guide/watching.md` "How diffing works" states: "Row identity is
  determined by position (row index within the file). If a file previously
  produced 3 rows and now produces 2, the first two rows are compared for
  updates and the third is emitted as a delete."
- `packages/core/src/differ.rs::diff_rows` does a **full replace** (delete all
  old rows, insert all new rows) whenever the new file has fewer rows than the
  old file. The third row is not selectively deleted; all three are deleted
  and the two remaining are re-inserted. Only when the file grows or stays the
  same length does positional identity apply (Updates for changed indices,
  Inserts for appended lines).
- The new test
  `test_docs_gaps.py::describe_watching_guide_positional_identity_gap::it_emits_delete_for_shrinking_file_positionally`
  passes by asserting end-state only (the dropped row's data is present in
  *some* delete event and the DB reflects only 2 rows), with an in-test
  comment explicitly noting the divergence.
- **Recommendation:** update `docs/guide/watching.md` "How diffing works" to
  describe the actual "full replace on shrink" behavior, or change
  `diff_rows` to match the documented positional-delete semantics. Either
  direction is reasonable; the docs-as-spec rule suggests picking the
  behavior we want and aligning the other side.

---

## 5. Cross-SDK Parity Gaps (surfaced, not fixed)

Discovered while mirroring the Python gap tests to Rust and TypeScript:

### 5.1 Rust `RowEvent` has no `file_path` on Insert/Update/Delete — CLOSED (dirsql-n7x)

- `docs/guide/watching.md` documents `RowEvent.file_path` as a relative
  path on all event variants.
- Previously the Rust SDK re-exported `dirsql::differ::RowEvent`, whose
  `Insert` / `Update` / `Delete` variants carried `{table, row}` only and
  only the `Error` variant had a `file_path` field.
- **Closed in dirsql-n7x**: `file_path: String` is now a field on
  `RowEvent::Insert`, `::Update`, and `::Delete` in `packages/rust/src/differ.rs`
  (Error keeps its existing `file_path: PathBuf`). The Rust SDK re-exports
  the enum directly, and the napi bindings for Python/TS now read
  `file_path` from the core event. Covered by
  `packages/rust/tests/docs_gaps.rs::watch_insert_event_carries_relative_file_path`.

### 5.2 TypeScript SDK `fromConfig` — CLOSED (dirsql-hh3)

- `docs/guide/config.md` documents `.dirsql.toml` driven `fromConfig`
  across Python and Rust.
- **Resolved** by bead `dirsql-hh3`: `packages/ts/ts/index.ts` now
  exposes `DirSQL.fromConfig(configPath)` as a static factory, backed
  by the core config loader + parser in `packages/ts/src/lib.rs`.
- TypeScript mirrors of the format tests (`.json`, `.jsonl`, `.ndjson`,
  `.csv`, `.tsv`, `.toml`, `.yaml`/`.yml`, `.md` frontmatter,
  path captures, column mapping, `each`, ignore, multiple tables,
  explicit format override, `strict = true`, and the error cases) now
  live in `packages/ts/test/from_config.test.ts`.
- Signature matches Python's (`configPath` is the path to the TOML
  file, not the root dir); this divergence from Rust's `from_config(root_dir)`
  is documented in `PARITY.md` under "Language-Idiomatic Exceptions".

---

## Verification

Commands run locally in this worktree:

- `uv run pytest packages/python/tests/integration/ -v` -> 85 passed (+11 new in `test_docs_gaps.py`)
- `cargo test --manifest-path packages/rust/Cargo.toml` -> all passing (+9 new in `docs_gaps.rs`)
- `cd packages/ts; pnpm test` -> 11 passed (+3 new in `index.test.ts`)

No test suite was modified except by additions. No docs, no source code, no
other tests were changed by this audit pass.
