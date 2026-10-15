### Shared row keys

#### Summary

Rows parsed from `on-file` output share one string per key. For Rust callers, `infer::JsonRow` holds `Vec<(Arc<str>, serde_json::Value)>` in place of `Vec<(String, serde_json::Value)>`. The CLI and the Python and TypeScript SDKs are unaffected.

#### Required changes

Rust code that builds a `JsonRow` by hand converts each key with `.into()`:

```rust
// before
JsonRow(vec![("name".to_string(), json!("a"))]);
// after
JsonRow(vec![("name".into(), json!("a"))]);
```

Code that reads a key as `&String` reads it as `&str` (`&**key`).

#### Deprecations removed

_None._

#### Behavior changes without code changes

_None._

#### Verification

```bash
dirsql query "SELECT count(*) FROM events" -c .dirsql.toml
```

Prints the same count as before, sooner.
