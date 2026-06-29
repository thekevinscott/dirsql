//! Native-language config support: spawn `dirsql interpret <path>` as a
//! subprocess and build a [`DirSQL`] whose `Table::extract` closures
//! NDJSON-RPC into it. V1 of the protocol is strictly sequential — one
//! outstanding extract request at a time (per #196).
//!
//! The Rust binary itself doesn't know how to read `.py` / `.{js,mjs,cjs}`
//! — that's the helper's job. The binary only cares about the handshake
//! shape and the extract protocol, both defined by #196.
//!
//! The pure protocol pieces ([`parse_handshake`], [`dispatch_extract`])
//! are separated from the subprocess plumbing in [`InterpretHelper`] so
//! the wire-format handling is exercised by in-process unit tests.

use std::collections::HashMap;
use std::io::{BufRead, BufReader, BufWriter, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, ChildStdout};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use serde::Deserialize;

use crate::db::parse_table_name;
use crate::{DirSQL, Extension, Row, Table, Value};

/// NDJSON helper subprocess + the shared IO it dispatches over.
pub struct InterpretHelper {
    /// Kept so the child process is killed when this struct is dropped.
    _child: Child,
    io: Arc<Mutex<HelperIo>>,
    next_id: AtomicU64,
}

struct HelperIo {
    stdin: BufWriter<ChildStdin>,
    stdout: BufReader<ChildStdout>,
}

/// Handshake payload sent on stdout once at startup. Each helper writes
/// `{"type":"config","state":{...}}\n` and flushes; that `state` is the
/// SDK's own serialization of `DirSQL` (`vars(app)` in Python, `app.toJSON()`
/// in TypeScript). The two SDKs differ in case (`persist_path` vs.
/// `persistPath`) so we accept either. The discriminator `type` field is
/// ignored — serde drops unknown fields by default.
#[derive(Deserialize)]
struct Handshake {
    state: HandshakeState,
}

#[derive(Deserialize)]
struct HandshakeState {
    root: String,
    tables: Vec<HandshakeTable>,
    #[serde(default)]
    ignore: Vec<String>,
    #[serde(default)]
    persist: bool,
    #[serde(default, alias = "persistPath")]
    persist_path: Option<String>,
    /// SQLite extensions the config declared (`{path, entrypoint?}` each).
    /// Defaults to empty so a handshake from an SDK that doesn't yet emit
    /// the key still parses. Paths are taken verbatim — the SDK already
    /// resolved config-relative paths before serializing the snapshot.
    #[serde(default)]
    extensions: Vec<HandshakeExtension>,
}

#[derive(Debug, Deserialize)]
struct HandshakeTable {
    ddl: String,
    glob: String,
    #[serde(default)]
    strict: bool,
}

#[derive(Debug, Deserialize)]
struct HandshakeExtension {
    path: String,
    #[serde(default)]
    entrypoint: Option<String>,
}

/// Response per extract request. The discriminator `type` field and the
/// echoed `id` field are ignored — V1 of the protocol is strictly
/// sequential, so the helper's reply is unambiguous.
#[derive(Deserialize)]
struct ExtractResponse {
    ok: bool,
    #[serde(default)]
    rows: Vec<HashMap<String, serde_json::Value>>,
    #[serde(default)]
    error: Option<String>,
}

impl InterpretHelper {
    /// Take ownership of an already-spawned child whose stdin / stdout
    /// are piped, read its handshake line, and wrap it in an
    /// `InterpretHelper`. The Command-building / process-spawning step
    /// lives in `bin/dirsql.rs` so the helper here can be exercised
    /// against any substitute subprocess (real `dirsql interpret`,
    /// fake bash helper, etc.).
    pub fn from_child(mut child: Child) -> Result<(Arc<Self>, NativeConfig), String> {
        let stdin = child
            .stdin
            .take()
            .ok_or_else(|| "failed to capture interpret stdin".to_string())?;
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| "failed to capture interpret stdout".to_string())?;
        let mut stdout = BufReader::new(stdout);

        let config = parse_handshake(&mut stdout)?;

        let helper = Arc::new(Self {
            _child: child,
            io: Arc::new(Mutex::new(HelperIo {
                stdin: BufWriter::new(stdin),
                stdout,
            })),
            next_id: AtomicU64::new(1),
        });

        Ok((helper, config))
    }

    /// Dispatch a single `extract` request and block for the response.
    fn extract(&self, table: &str, path: &str) -> Result<Vec<Row>, String> {
        let id = self.next_id.fetch_add(1, Ordering::SeqCst);
        let mut io = self
            .io
            .lock()
            .map_err(|e| format!("interpret IO mutex poisoned: {e}"))?;
        let HelperIo { stdin, stdout } = &mut *io;
        dispatch_extract(stdout, stdin, id, table, path)
    }
}

/// Read one handshake line and parse it into the resolved
/// [`NativeConfig`]. Split out from [`InterpretHelper::spawn`] so the
/// parser is testable against in-memory bytes — every error branch
/// (early EOF, malformed JSON) is covered by unit tests. Takes the
/// reader as `&mut dyn BufRead` rather than a generic parameter so the
/// production (`BufReader<ChildStdout>`) and test (`Cursor<&[u8]>`)
/// call sites share a single monomorphized definition.
fn parse_handshake(stdout: &mut dyn BufRead) -> Result<NativeConfig, String> {
    let mut line = String::new();
    stdout
        .read_line(&mut line)
        .map_err(|e| format!("failed to read interpret handshake: {e}"))?;
    if line.is_empty() {
        return Err("interpret exited before sending handshake".into());
    }

    let handshake: Handshake = serde_json::from_str(line.trim())
        .map_err(|e| format!("invalid interpret handshake: {e}; line was: {line:?}"))?;

    let state = handshake.state;
    Ok(NativeConfig {
        root: PathBuf::from(state.root),
        tables: state.tables,
        ignore: state.ignore,
        persist: state.persist,
        persist_path: state.persist_path.map(PathBuf::from),
        extensions: state
            .extensions
            .into_iter()
            .map(|e| Extension {
                path: PathBuf::from(e.path),
                entrypoint: e.entrypoint,
            })
            .collect(),
    })
}

/// Write one extract request, read one response, return the parsed
/// rows. Split out from [`InterpretHelper::extract`] so the protocol is
/// testable against in-memory streams. Takes the streams as
/// `&mut dyn` rather than generics so production and test call sites
/// share a single monomorphized definition.
fn dispatch_extract(
    stdout: &mut dyn BufRead,
    stdin: &mut dyn Write,
    id: u64,
    table: &str,
    path: &str,
) -> Result<Vec<Row>, String> {
    let req = serde_json::json!({
        "type": "extract",
        "id": id,
        "table": table,
        "path": path,
    });
    writeln!(stdin, "{req}").map_err(|e| format!("write to interpret: {e}"))?;
    stdin
        .flush()
        .map_err(|e| format!("flush interpret stdin: {e}"))?;

    let mut buf = String::new();
    stdout
        .read_line(&mut buf)
        .map_err(|e| format!("read from interpret: {e}"))?;
    if buf.is_empty() {
        return Err("interpret closed stdout mid-request".into());
    }

    let resp: ExtractResponse = serde_json::from_str(buf.trim())
        .map_err(|e| format!("invalid interpret response: {e}; line was: {buf:?}"))?;

    if !resp.ok {
        return Err(resp
            .error
            .unwrap_or_else(|| "interpret extract failed".into()));
    }

    Ok(resp.rows.into_iter().map(json_row_to_value_row).collect())
}

/// Parsed handshake state — the caller turns this into a [`DirSQL`].
#[derive(Debug)]
pub struct NativeConfig {
    pub root: PathBuf,
    tables: Vec<HandshakeTable>,
    pub ignore: Vec<String>,
    pub persist: bool,
    pub persist_path: Option<PathBuf>,
    /// SQLite extensions to load onto the connection at startup. Resolved
    /// verbatim from the handshake (the SDK already merged config-file and
    /// programmatic entries and resolved relative paths). See [`Extension`].
    pub extensions: Vec<Extension>,
}

/// Build a [`DirSQL`] from a spawned interpret helper. Each table's
/// `extract` closure dispatches via NDJSON to the helper.
pub fn build_dirsql(helper: Arc<InterpretHelper>, config: NativeConfig) -> Result<DirSQL, String> {
    let mut tables = Vec::with_capacity(config.tables.len());
    for ht in config.tables {
        let table_name = parse_table_name(&ht.ddl).ok_or_else(|| {
            format!(
                "interpret handshake: could not parse table name from DDL: {}",
                ht.ddl
            )
        })?;
        let h = helper.clone();
        let extract =
            move |path: &str| -> Result<Vec<Row>, Box<dyn std::error::Error + Send + Sync>> {
                h.extract(&table_name, path).map_err(|e| e.into())
            };
        let mut table = Table::try_new(ht.ddl, ht.glob, extract);
        table.strict = ht.strict;
        tables.push(table);
    }

    let mut builder = DirSQL::builder()
        .root(config.root)
        .tables(tables)
        .ignore(config.ignore)
        .extensions(config.extensions);
    if config.persist {
        builder = builder.persist(true);
    }
    if let Some(p) = config.persist_path {
        builder = builder.persist_path(p);
    }
    builder.build().map_err(|e| e.to_string())
}

/// Convert a JSON object row to the internal `Row` shape (`HashMap<String, Value>`).
fn json_row_to_value_row(obj: HashMap<String, serde_json::Value>) -> Row {
    obj.into_iter()
        .map(|(k, v)| (k, json_to_value(v)))
        .collect()
}

fn json_to_value(v: serde_json::Value) -> Value {
    match v {
        serde_json::Value::Null => Value::Null,
        serde_json::Value::Bool(b) => Value::Integer(if b { 1 } else { 0 }),
        serde_json::Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                Value::Integer(i)
            } else if let Some(f) = n.as_f64() {
                Value::Real(f)
            } else {
                Value::Text(n.to_string())
            }
        }
        serde_json::Value::String(s) => Value::Text(s),
        // Arrays and objects round-trip as their JSON repr; SQLite is typed
        // and the user's extract is expected to return scalar columns.
        other => Value::Text(other.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    // The subprocess-driven tests (`InterpretHelper::from_child` against a
    // real `bash`/`true` child, and the `build_dirsql` round-trips that
    // spawn a fake helper and write real fixture files) live in
    // `tests/native_config.rs`. They exercise effectful std
    // (`std::process::Command`, `std::fs::write`) and so belong at the
    // integration tier per the `testing-conventions` `unit lint` isolation
    // rule. The pure wire-format tests below operate on in-memory
    // `Cursor`/`Vec` streams and stay inline next to the private functions
    // they cover.

    // -- parse_handshake -----------------------------------------------

    #[test]
    fn parse_handshake_returns_resolved_config_for_minimal_input() {
        let line = br#"{"type":"config","state":{"root":"/x","tables":[{"ddl":"CREATE TABLE t (a TEXT)","glob":"*.json"}]}}
"#;
        let mut reader = Cursor::new(line.as_slice());
        let cfg = parse_handshake(&mut reader).unwrap();
        assert_eq!(cfg.root, PathBuf::from("/x"));
        assert_eq!(cfg.tables.len(), 1);
        assert_eq!(cfg.tables[0].ddl, "CREATE TABLE t (a TEXT)");
        assert_eq!(cfg.tables[0].glob, "*.json");
        assert!(!cfg.tables[0].strict);
        assert!(cfg.ignore.is_empty());
        assert!(!cfg.persist);
        assert!(cfg.persist_path.is_none());
    }

    #[test]
    fn parse_handshake_accepts_snake_case_persist_path() {
        let line = br#"{"type":"config","state":{"root":"/r","tables":[],"persist":true,"persist_path":"/cache"}}
"#;
        let cfg = parse_handshake(&mut Cursor::new(line.as_slice())).unwrap();
        assert!(cfg.persist);
        assert_eq!(cfg.persist_path, Some(PathBuf::from("/cache")));
    }

    #[test]
    fn parse_handshake_accepts_camel_case_persist_path() {
        let line = br#"{"type":"config","state":{"root":"/r","tables":[],"persist":true,"persistPath":"/cache"}}
"#;
        let cfg = parse_handshake(&mut Cursor::new(line.as_slice())).unwrap();
        assert!(cfg.persist);
        assert_eq!(cfg.persist_path, Some(PathBuf::from("/cache")));
    }

    #[test]
    fn parse_handshake_carries_ignore_strict_and_extras() {
        let line = br#"{"type":"config","state":{"root":"/r","tables":[{"ddl":"CREATE TABLE s (a TEXT)","glob":"x","strict":true}],"ignore":["a","b"]}}
"#;
        let cfg = parse_handshake(&mut Cursor::new(line.as_slice())).unwrap();
        assert!(cfg.tables[0].strict);
        assert_eq!(cfg.ignore, vec!["a".to_string(), "b".to_string()]);
    }

    #[test]
    fn parse_handshake_carries_extensions_with_path_and_entrypoint() {
        // A `.py` / `.js` config that declares `extensions=[...]` serializes
        // them into the handshake `state`; the parser must carry both `path`
        // and the optional `entrypoint` through to the resolved config (#229).
        let line = br#"{"type":"config","state":{"root":"/r","tables":[],"extensions":[{"path":"/ext/vec0.so","entrypoint":"sqlite3_vec_init"}]}}
"#;
        let cfg = parse_handshake(&mut Cursor::new(line.as_slice())).unwrap();
        assert_eq!(cfg.extensions.len(), 1);
        assert_eq!(cfg.extensions[0].path, PathBuf::from("/ext/vec0.so"));
        assert_eq!(
            cfg.extensions[0].entrypoint.as_deref(),
            Some("sqlite3_vec_init"),
        );
    }

    #[test]
    fn parse_handshake_extension_entrypoint_is_optional() {
        let line = br#"{"type":"config","state":{"root":"/r","tables":[],"extensions":[{"path":"/ext/a.so"}]}}
"#;
        let cfg = parse_handshake(&mut Cursor::new(line.as_slice())).unwrap();
        assert_eq!(cfg.extensions.len(), 1);
        assert!(cfg.extensions[0].entrypoint.is_none());
    }

    #[test]
    fn parse_handshake_defaults_extensions_to_empty_when_absent() {
        let line = br#"{"type":"config","state":{"root":"/r","tables":[]}}
"#;
        let cfg = parse_handshake(&mut Cursor::new(line.as_slice())).unwrap();
        assert!(cfg.extensions.is_empty());
    }

    #[test]
    fn parse_handshake_errors_on_empty_stream() {
        let mut reader = Cursor::new(b"".as_slice());
        let err = parse_handshake(&mut reader).unwrap_err();
        assert!(
            err.contains("exited before sending handshake"),
            "got: {err}"
        );
    }

    #[test]
    fn parse_handshake_errors_on_malformed_json() {
        let mut reader = Cursor::new(b"not-json\n".as_slice());
        let err = parse_handshake(&mut reader).unwrap_err();
        assert!(err.contains("invalid interpret handshake"), "got: {err}");
    }

    // -- dispatch_extract ----------------------------------------------

    #[test]
    fn dispatch_extract_writes_request_and_parses_ok_response() {
        let response = br#"{"type":"result","id":1,"ok":true,"rows":[{"title":"Alpha"}]}
"#;
        let mut stdout = Cursor::new(response.as_slice());
        let mut stdin: Vec<u8> = Vec::new();

        let rows = dispatch_extract(&mut stdout, &mut stdin, 1, "papers", "/x.json").unwrap();

        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].get("title"), Some(&Value::Text("Alpha".into())));

        let req = String::from_utf8(stdin).unwrap();
        assert!(req.contains("\"type\":\"extract\""));
        assert!(req.contains("\"id\":1"));
        assert!(req.contains("\"table\":\"papers\""));
        assert!(req.contains("\"path\":\"/x.json\""));
        assert!(req.ends_with('\n'));
    }

    #[test]
    fn dispatch_extract_returns_err_when_response_ok_false() {
        let response = br#"{"type":"result","id":1,"ok":false,"error":"boom"}
"#;
        let mut stdout = Cursor::new(response.as_slice());
        let mut stdin: Vec<u8> = Vec::new();
        let err = dispatch_extract(&mut stdout, &mut stdin, 1, "t", "p").unwrap_err();
        assert_eq!(err, "boom");
    }

    #[test]
    fn dispatch_extract_falls_back_to_generic_message_when_error_is_missing() {
        let response = br#"{"type":"result","id":1,"ok":false}
"#;
        let mut stdout = Cursor::new(response.as_slice());
        let mut stdin: Vec<u8> = Vec::new();
        let err = dispatch_extract(&mut stdout, &mut stdin, 1, "t", "p").unwrap_err();
        assert!(err.contains("interpret extract failed"), "got: {err}");
    }

    #[test]
    fn dispatch_extract_errors_when_response_is_empty() {
        let mut stdout = Cursor::new(b"".as_slice());
        let mut stdin: Vec<u8> = Vec::new();
        let err = dispatch_extract(&mut stdout, &mut stdin, 1, "t", "p").unwrap_err();
        assert!(err.contains("closed stdout mid-request"), "got: {err}");
    }

    #[test]
    fn dispatch_extract_errors_on_malformed_response() {
        let mut stdout = Cursor::new(b"not-json\n".as_slice());
        let mut stdin: Vec<u8> = Vec::new();
        let err = dispatch_extract(&mut stdout, &mut stdin, 1, "t", "p").unwrap_err();
        assert!(err.contains("invalid interpret response"), "got: {err}");
    }

    #[test]
    fn dispatch_extract_returns_empty_rows_on_ok_response_without_rows() {
        let response = br#"{"type":"result","id":1,"ok":true}
"#;
        let mut stdout = Cursor::new(response.as_slice());
        let mut stdin: Vec<u8> = Vec::new();
        let rows = dispatch_extract(&mut stdout, &mut stdin, 1, "t", "p").unwrap();
        assert!(rows.is_empty());
    }

    // -- json_to_value -------------------------------------------------

    #[test]
    fn json_to_value_maps_each_variant() {
        assert!(matches!(
            json_to_value(serde_json::Value::Null),
            Value::Null
        ));
        assert!(matches!(
            json_to_value(serde_json::Value::Bool(true)),
            Value::Integer(1),
        ));
        assert!(matches!(
            json_to_value(serde_json::Value::Bool(false)),
            Value::Integer(0),
        ));
        assert!(matches!(
            json_to_value(serde_json::json!(42i64)),
            Value::Integer(42),
        ));
        match json_to_value(serde_json::json!(1.5f64)) {
            Value::Real(f) => assert!((f - 1.5).abs() < f64::EPSILON),
            other => panic!("expected Real, got {other:?}"),
        }
        assert!(matches!(
            json_to_value(serde_json::json!("hello")),
            Value::Text(ref s) if s == "hello",
        ));
        // Arrays and objects fall through to their JSON repr.
        assert!(matches!(
            json_to_value(serde_json::json!([1, 2])),
            Value::Text(ref s) if s == "[1,2]",
        ));
        assert!(matches!(
            json_to_value(serde_json::json!({"k":1})),
            Value::Text(ref s) if s == r#"{"k":1}"#,
        ));
    }

    #[test]
    fn json_row_to_value_row_maps_each_field() {
        let mut obj = HashMap::new();
        obj.insert("a".into(), serde_json::json!("x"));
        obj.insert("b".into(), serde_json::json!(7));
        let row = json_row_to_value_row(obj);
        assert_eq!(row.get("a"), Some(&Value::Text("x".into())));
        assert_eq!(row.get("b"), Some(&Value::Integer(7)));
    }

    // -- build_dirsql --------------------------------------------------
    //
    // The `build_dirsql` round-trips and the `InterpretHelper::from_child`
    // success/error paths spawn a real subprocess (`bash`/`true`) and write
    // real fixture files, so they live in `tests/native_config.rs`.

    /// A writer that fails every write with the given `io::ErrorKind`,
    /// used to exercise `dispatch_extract`'s IO-error arms.
    struct FailingWriter(std::io::ErrorKind);
    impl Write for FailingWriter {
        fn write(&mut self, _: &[u8]) -> std::io::Result<usize> {
            Err(std::io::Error::new(self.0, "induced"))
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Err(std::io::Error::new(self.0, "induced"))
        }
    }

    #[test]
    fn dispatch_extract_surfaces_write_errors() {
        let response = b"";
        let mut stdout = Cursor::new(response.as_slice());
        let mut stdin = FailingWriter(std::io::ErrorKind::BrokenPipe);
        let err = dispatch_extract(&mut stdout, &mut stdin, 1, "t", "p").unwrap_err();
        assert!(err.contains("write to interpret"), "got: {err}");
    }

    /// A reader that fails read_line with the given `io::ErrorKind`,
    /// used to exercise `parse_handshake` / `dispatch_extract`'s read
    /// error arms.
    struct FailingReader(std::io::ErrorKind);
    impl std::io::Read for FailingReader {
        fn read(&mut self, _: &mut [u8]) -> std::io::Result<usize> {
            Err(std::io::Error::new(self.0, "induced"))
        }
    }
    impl BufRead for FailingReader {
        fn fill_buf(&mut self) -> std::io::Result<&[u8]> {
            Err(std::io::Error::new(self.0, "induced"))
        }
        fn consume(&mut self, _: usize) {}
    }

    #[test]
    fn parse_handshake_surfaces_read_errors() {
        let mut reader = FailingReader(std::io::ErrorKind::ConnectionReset);
        let err = parse_handshake(&mut reader).unwrap_err();
        assert!(
            err.contains("failed to read interpret handshake"),
            "got: {err}"
        );
    }

    #[test]
    fn dispatch_extract_surfaces_read_errors() {
        let mut stdout = FailingReader(std::io::ErrorKind::ConnectionReset);
        let mut stdin: Vec<u8> = Vec::new();
        let err = dispatch_extract(&mut stdout, &mut stdin, 1, "t", "p").unwrap_err();
        assert!(err.contains("read from interpret"), "got: {err}");
    }
}
