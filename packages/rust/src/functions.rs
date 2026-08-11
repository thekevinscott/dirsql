//! Worker-backed SQL scalar functions (`[[dirsql.function]]`).
//!
//! Each declared function is registered on the connection via rusqlite's
//! `create_scalar_function`, once per accepted arity. Registration is inert:
//! the worker process is spawned lazily on the function's FIRST call, kept
//! alive for the rest of the invocation (one process total, never one per
//! row), and torn down when the index is dropped. A function nobody calls
//! costs nothing.
//!
//! ## Protocol
//!
//! Newline-delimited JSON over the worker's stdin/stdout, one round-trip per
//! call:
//!
//! - Request: `{"call": [<arg>, ...]}` — SQL TEXT as a JSON string,
//!   INTEGER/REAL as JSON numbers, NULL as `null`, BLOB as
//!   `{"$bytes": "<base64>"}`.
//! - Response: `{"ok": <value>}` (same scalar encodings; a JSON array or any
//!   other object is bound as TEXT, its JSON text) or `{"err": "message"}`,
//!   which fails the query with that message.
//! - The worker's stderr is inherited, passing straight through to dirsql's
//!   stderr.
//!
//! Each round-trip is bounded by the function's per-call timeout (its
//! `timeout` key, else the 30-second default — [`DEFAULT_FUNCTION_TIMEOUT`]).
//! A timeout or a worker crash kills the worker and fails the query with an
//! actionable error; the next call starts a fresh worker.

use std::io::{BufRead, BufReader, Write};
use std::path::PathBuf;
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::mpsc::{Receiver, RecvTimeoutError, SyncSender};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use rusqlite::Connection;
use rusqlite::functions::FunctionFlags;

use crate::db::Value;

/// The per-call timeout when a `[[dirsql.function]]` entry declares no
/// `timeout` of its own. A round-trip on a persistent worker cannot be
/// bounded by wrapping the command in `timeout(1)`, so the mechanism carries
/// its own default.
pub const DEFAULT_FUNCTION_TIMEOUT: Duration = Duration::from_secs(30);

/// A `[[dirsql.function]]` entry with its per-config context resolved: the
/// worker's working directory (the config file's parent) and the effective
/// per-call timeout (the entry's own `timeout`, else
/// [`DEFAULT_FUNCTION_TIMEOUT`]).
#[doc(hidden)]
pub struct ResolvedFunction {
    pub name: String,
    pub args: Vec<u8>,
    pub command: String,
    pub deterministic: bool,
    pub timeout: Duration,
    pub cwd: PathBuf,
}

/// Register every resolved function on `conn`, once per accepted arity.
/// Purely registration — no worker is spawned here.
pub(crate) fn register_all(
    conn: &Connection,
    functions: &[ResolvedFunction],
) -> rusqlite::Result<()> {
    for function in functions {
        let worker = Arc::new(Worker::for_process(function));
        for &arity in &function.args {
            let worker = Arc::clone(&worker);
            conn.create_scalar_function(
                &function.name,
                i32::from(arity),
                function_flags(function.deterministic),
                move |ctx| {
                    let mut args = Vec::with_capacity(ctx.len());
                    for i in 0..ctx.len() {
                        args.push(Value::from(rusqlite::types::Value::from(ctx.get_raw(i))));
                    }
                    worker
                        .call(&args)
                        .map_err(|message| rusqlite::Error::UserFunctionError(message.into()))
                },
            )?;
        }
    }
    Ok(())
}

/// The registration flags for a declared function: UTF-8 always, plus
/// `SQLITE_DETERMINISTIC` when the entry opts in.
fn function_flags(deterministic: bool) -> FunctionFlags {
    if deterministic {
        FunctionFlags::SQLITE_UTF8 | FunctionFlags::SQLITE_DETERMINISTIC
    } else {
        FunctionFlags::SQLITE_UTF8
    }
}

/// How a worker round-trip can fail below the protocol layer.
enum TransportError {
    /// No response within the per-call timeout.
    Timeout,
    /// The worker exited (or closed its pipes) before replying.
    Closed,
}

/// The seam between the call state machine and the worker process: send one
/// request line, receive one response line. Production uses
/// [`ProcessTransport`]; unit tests inject a scripted double.
trait Transport: Send {
    fn send_line(&mut self, line: &str) -> Result<(), TransportError>;
    fn recv_line(&mut self, timeout: Duration) -> Result<String, TransportError>;
}

type Spawner = Box<dyn Fn() -> Result<Box<dyn Transport>, String> + Send>;

/// One declared function's lazy persistent worker. Calls are serialized by
/// the inner mutex (SQLite invokes scalar functions one at a time per
/// connection anyway); the transport is created on the first call and
/// dropped — killing the process — on teardown or after a failure.
pub(crate) struct Worker {
    name: String,
    command: String,
    timeout: Duration,
    inner: Mutex<WorkerInner>,
}

struct WorkerInner {
    spawner: Spawner,
    transport: Option<Box<dyn Transport>>,
}

impl Worker {
    fn for_process(function: &ResolvedFunction) -> Self {
        let command = function.command.clone();
        let cwd = function.cwd.clone();
        Self::with_spawner(
            &function.name,
            &function.command,
            function.timeout,
            Box::new(move || spawn_process(&command, &cwd)),
        )
    }

    fn with_spawner(name: &str, command: &str, timeout: Duration, spawner: Spawner) -> Self {
        Self {
            name: name.to_string(),
            command: command.to_string(),
            timeout,
            inner: Mutex::new(WorkerInner {
                spawner,
                transport: None,
            }),
        }
    }

    /// One protocol round-trip: spawn the worker if this is the first call,
    /// send the encoded request, wait up to the per-call timeout for the
    /// response, and decode it. `Err` carries the message the query fails
    /// with. A transport failure (spawn error, crash, timeout) drops the
    /// worker so the next call starts fresh; a protocol-level `{"err": ...}`
    /// leaves the healthy worker running.
    fn call(&self, args: &[Value]) -> Result<Value, String> {
        let mut inner = self
            .inner
            .lock()
            .map_err(|e| format!("function `{}` worker state poisoned: {e}", self.name))?;

        if inner.transport.is_none() {
            let transport = (inner.spawner)().map_err(|e| {
                format!(
                    "failed to start worker for function `{}` (command `{}`): {e}",
                    self.name, self.command
                )
            })?;
            inner.transport = Some(transport);
        }
        let transport = inner.transport.as_mut().expect("spawned above");

        let request = request_line(args);
        if transport.send_line(&request).is_err() {
            inner.transport = None;
            return Err(format!(
                "worker for function `{}` (command `{}`) is not accepting requests; \
                 it may have exited — check its stderr above",
                self.name, self.command
            ));
        }

        match transport.recv_line(self.timeout) {
            Ok(line) => self.decode(&line),
            Err(TransportError::Timeout) => {
                inner.transport = None;
                Err(format!(
                    "call to function `{}` timed out after {:?} (worker command `{}`); \
                     raise the function's `timeout` if the worker legitimately needs \
                     longer per call",
                    self.name, self.timeout, self.command
                ))
            }
            Err(TransportError::Closed) => {
                inner.transport = None;
                Err(format!(
                    "worker for function `{}` (command `{}`) exited before replying — \
                     check its stderr above",
                    self.name, self.command
                ))
            }
        }
    }

    /// Decode one response line: `{"ok": <value>}` or `{"err": "message"}`.
    fn decode(&self, line: &str) -> Result<Value, String> {
        match parse_response(line) {
            Ok(Response::Ok(value)) => Ok(value),
            Ok(Response::Err(message)) => Err(message),
            Err(defect) => Err(format!(
                "function `{}` worker sent an invalid response ({defect}): {line}",
                self.name
            )),
        }
    }
}

/// Encode one request: `{"call": [...]}` with the wire encodings from the
/// module docs.
fn request_line(args: &[Value]) -> String {
    let encoded: Vec<serde_json::Value> = args.iter().map(value_to_json).collect();
    serde_json::json!({ "call": encoded }).to_string()
}

/// SQL value → wire JSON: TEXT as string, INTEGER/REAL as numbers, NULL as
/// null, BLOB as `{"$bytes": "<base64>"}`. A non-finite REAL has no JSON
/// number representation and encodes as null.
fn value_to_json(value: &Value) -> serde_json::Value {
    match value {
        Value::Null => serde_json::Value::Null,
        Value::Integer(i) => serde_json::Value::from(*i),
        Value::Real(f) => serde_json::Number::from_f64(*f)
            .map(serde_json::Value::Number)
            .unwrap_or(serde_json::Value::Null),
        Value::Text(s) => serde_json::Value::from(s.as_str()),
        Value::Blob(bytes) => serde_json::json!({ "$bytes": base64_encode(bytes) }),
    }
}

#[derive(Debug)]
enum Response {
    Ok(Value),
    Err(String),
}

/// Parse one response line. `Err` names the defect (invalid JSON, neither
/// `ok` nor `err`, a bad `$bytes` payload) for the caller to wrap.
fn parse_response(line: &str) -> Result<Response, String> {
    let parsed: serde_json::Value =
        serde_json::from_str(line).map_err(|e| format!("invalid JSON: {e}"))?;
    let object = parsed
        .as_object()
        .ok_or_else(|| "expected a JSON object".to_string())?;
    if let Some(message) = object.get("err") {
        let message = message
            .as_str()
            .ok_or_else(|| "\"err\" must be a string".to_string())?;
        return Ok(Response::Err(message.to_string()));
    }
    let value = object
        .get("ok")
        .ok_or_else(|| "expected an \"ok\" or \"err\" key".to_string())?;
    Ok(Response::Ok(json_to_sql_value(value)?))
}

/// Wire JSON → SQL value: string → TEXT, integral number → INTEGER, other
/// number → REAL, null → NULL, bool → INTEGER 0/1,
/// `{"$bytes": "<base64>"}` → BLOB. Any other array/object is bound as TEXT
/// (its JSON text) so e.g. embedding vectors feed sqlite-vec's distance
/// functions directly.
fn json_to_sql_value(value: &serde_json::Value) -> Result<Value, String> {
    match value {
        serde_json::Value::Null => Ok(Value::Null),
        serde_json::Value::Bool(b) => Ok(Value::Integer(i64::from(*b))),
        serde_json::Value::Number(n) => Ok(match n.as_i64() {
            Some(i) => Value::Integer(i),
            None => Value::Real(n.as_f64().unwrap_or(f64::NAN)),
        }),
        serde_json::Value::String(s) => Ok(Value::Text(s.clone())),
        serde_json::Value::Object(map) if map.len() == 1 && map.contains_key("$bytes") => {
            let encoded = map["$bytes"]
                .as_str()
                .ok_or_else(|| "\"$bytes\" must be a base64 string".to_string())?;
            let bytes = base64_decode(encoded)
                .ok_or_else(|| "\"$bytes\" is not valid base64".to_string())?;
            Ok(Value::Blob(bytes))
        }
        other => Ok(Value::Text(other.to_string())),
    }
}

const BASE64_ALPHABET: &[u8; 64] =
    b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

/// Standard base64 with padding. Hand-rolled (~20 lines) rather than a new
/// dependency for one wire field.
fn base64_encode(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let b0 = u32::from(chunk[0]);
        let b1 = chunk.get(1).map(|b| u32::from(*b));
        let b2 = chunk.get(2).map(|b| u32::from(*b));
        let triple = (b0 << 16) | (b1.unwrap_or(0) << 8) | b2.unwrap_or(0);
        let index = |shift: u32| BASE64_ALPHABET[(triple >> shift & 0x3f) as usize] as char;
        out.push(index(18));
        out.push(index(12));
        out.push(if b1.is_some() { index(6) } else { '=' });
        out.push(if b2.is_some() { index(0) } else { '=' });
    }
    out
}

/// Decode standard base64 (padding required). `None` on any malformed input.
fn base64_decode(text: &str) -> Option<Vec<u8>> {
    let bytes = text.as_bytes();
    if !bytes.len().is_multiple_of(4) {
        return None;
    }
    let value_of = |b: u8| -> Option<u32> {
        BASE64_ALPHABET
            .iter()
            .position(|c| *c == b)
            .map(|i| u32::try_from(i).expect("index < 64"))
    };
    let mut out = Vec::with_capacity(bytes.len() / 4 * 3);
    for (i, chunk) in bytes.chunks(4).enumerate() {
        let last = (i + 1) * 4 == bytes.len();
        let pad = chunk.iter().filter(|b| **b == b'=').count();
        // Padding may only close the final chunk, as its last one or two chars.
        if pad > 0 && (!last || pad > 2 || chunk[..4 - pad].contains(&b'=')) {
            return None;
        }
        let mut triple: u32 = 0;
        for b in &chunk[..4 - pad] {
            triple = (triple << 6) | value_of(*b)?;
        }
        triple <<= 6 * u32::try_from(pad).expect("pad <= 2");
        #[expect(
            clippy::cast_possible_truncation,
            reason = "each shift isolates one byte"
        )]
        {
            out.push((triple >> 16) as u8);
            if pad < 2 {
                out.push((triple >> 8) as u8);
            }
            if pad < 1 {
                out.push(triple as u8);
            }
        }
    }
    Some(out)
}

/// The production transport: the spawned worker process, its piped stdin,
/// and a reader thread draining its stdout line-by-line into a channel (a
/// blocking read has no timeout; `recv_timeout` on the channel does).
struct ProcessTransport {
    child: Child,
    stdin: ChildStdin,
    lines: Receiver<String>,
}

/// Spawn the worker: argv-split (shell-like quoting, no shell), run from the
/// config file's directory, stdin/stdout piped for the protocol, stderr
/// inherited so worker diagnostics pass straight through.
fn spawn_process(command: &str, cwd: &std::path::Path) -> Result<Box<dyn Transport>, String> {
    let argv = crate::command::build_argv(command, &[]).map_err(|e| e.to_string())?;
    let mut child = Command::new(&argv[0])
        .args(&argv[1..])
        .current_dir(cwd)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .spawn()
        .map_err(|e| e.to_string())?;

    let stdin = child.stdin.take().expect("stdin piped");
    let stdout = child.stdout.take().expect("stdout piped");
    // Rendezvous channel: the reader thread holds at most the one in-flight
    // response and exits on EOF (worker death or our kill dropping the pipe).
    let (tx, lines): (SyncSender<String>, Receiver<String>) = std::sync::mpsc::sync_channel(0);
    std::thread::spawn(move || {
        let reader = BufReader::new(stdout);
        for line in reader.lines() {
            let Ok(line) = line else { break };
            if tx.send(line).is_err() {
                break;
            }
        }
    });

    Ok(Box::new(ProcessTransport {
        child,
        stdin,
        lines,
    }))
}

impl Transport for ProcessTransport {
    fn send_line(&mut self, line: &str) -> Result<(), TransportError> {
        self.stdin
            .write_all(line.as_bytes())
            .and_then(|()| self.stdin.write_all(b"\n"))
            .and_then(|()| self.stdin.flush())
            .map_err(|_| TransportError::Closed)
    }

    fn recv_line(&mut self, timeout: Duration) -> Result<String, TransportError> {
        match self.lines.recv_timeout(timeout) {
            Ok(line) => Ok(line),
            Err(RecvTimeoutError::Timeout) => Err(TransportError::Timeout),
            Err(RecvTimeoutError::Disconnected) => Err(TransportError::Closed),
        }
    }
}

impl Drop for ProcessTransport {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn text(s: &str) -> Value {
        Value::Text(s.to_string())
    }

    // --- wire encoding -----------------------------------------------------

    #[test]
    fn request_line_encodes_every_scalar_shape() {
        let args = [
            text("txt"),
            Value::Integer(1),
            Value::Real(2.5),
            Value::Null,
            Value::Blob(vec![1, 2]),
        ];
        assert_eq!(
            request_line(&args),
            r#"{"call":["txt",1,2.5,null,{"$bytes":"AQI="}]}"#
        );
    }

    #[test]
    fn request_line_with_no_args_is_an_empty_call() {
        assert_eq!(request_line(&[]), r#"{"call":[]}"#);
    }

    #[test]
    fn value_to_json_encodes_nonfinite_real_as_null() {
        assert!(value_to_json(&Value::Real(f64::NAN)).is_null());
    }

    // --- response decoding -------------------------------------------------

    fn ok_value(line: &str) -> Value {
        match parse_response(line).unwrap() {
            Response::Ok(v) => v,
            Response::Err(m) => panic!("expected ok, got err {m:?}"),
        }
    }

    #[test]
    fn response_scalars_map_to_sql_values() {
        assert_eq!(ok_value(r#"{"ok": "hi"}"#), text("hi"));
        assert_eq!(ok_value(r#"{"ok": 7}"#), Value::Integer(7));
        assert_eq!(ok_value(r#"{"ok": 2.5}"#), Value::Real(2.5));
        assert_eq!(ok_value(r#"{"ok": null}"#), Value::Null);
        assert_eq!(ok_value(r#"{"ok": true}"#), Value::Integer(1));
        assert_eq!(
            ok_value(r#"{"ok": {"$bytes": "AQI="}}"#),
            Value::Blob(vec![1, 2])
        );
    }

    #[test]
    fn response_arrays_and_objects_bind_as_json_text() {
        assert_eq!(ok_value(r#"{"ok": [1.5, 2.0]}"#), text("[1.5,2.0]"));
        assert_eq!(ok_value(r#"{"ok": {"a": 1}}"#), text(r#"{"a":1}"#));
    }

    #[test]
    fn response_err_carries_the_message() {
        match parse_response(r#"{"err": "boom"}"#).unwrap() {
            Response::Err(m) => assert_eq!(m, "boom"),
            Response::Ok(v) => panic!("expected err, got ok {v:?}"),
        }
    }

    #[test]
    fn response_defects_are_named() {
        assert!(
            parse_response("not json")
                .unwrap_err()
                .contains("invalid JSON")
        );
        assert!(parse_response("[]").unwrap_err().contains("JSON object"));
        assert!(
            parse_response(r#"{"neither": 1}"#)
                .unwrap_err()
                .contains("\"ok\" or \"err\"")
        );
        assert!(
            parse_response(r#"{"err": 5}"#)
                .unwrap_err()
                .contains("must be a string")
        );
        assert!(
            parse_response(r#"{"ok": {"$bytes": "!!"}}"#)
                .unwrap_err()
                .contains("base64")
        );
        assert!(
            parse_response(r#"{"ok": {"$bytes": 3}}"#)
                .unwrap_err()
                .contains("base64 string")
        );
    }

    #[test]
    fn a_two_key_object_containing_bytes_is_not_a_blob() {
        assert_eq!(
            ok_value(r#"{"ok": {"$bytes": "AQI=", "x": 1}}"#),
            text(r#"{"$bytes":"AQI=","x":1}"#)
        );
    }

    // --- base64 ------------------------------------------------------------

    #[test]
    fn base64_round_trips_every_padding_length() {
        for bytes in [
            &b""[..],
            &b"f"[..],
            &b"fo"[..],
            &b"foo"[..],
            &b"foob"[..],
            &[0u8, 255, 16, 3][..],
        ] {
            let encoded = base64_encode(bytes);
            assert_eq!(base64_decode(&encoded).as_deref(), Some(bytes), "{encoded}");
        }
    }

    #[test]
    fn base64_known_vectors() {
        assert_eq!(base64_encode(b"foobar"), "Zm9vYmFy");
        assert_eq!(base64_encode(b"foob"), "Zm9vYg==");
        assert_eq!(base64_encode(&[1, 2]), "AQI=");
        assert_eq!(base64_decode("Zm9vYmFy").as_deref(), Some(&b"foobar"[..]));
    }

    #[test]
    fn base64_decode_rejects_malformed_input() {
        assert_eq!(base64_decode("abc"), None); // not a multiple of 4
        assert_eq!(base64_decode("a!=="), None); // bad alphabet
        assert_eq!(base64_decode("ab=c"), None); // padding inside a chunk
        assert_eq!(base64_decode("ab==cd=="), None); // padding before the last chunk
        assert_eq!(base64_decode("a==="), None); // over-padding
    }

    // --- defaults -----------------------------------------------------------

    #[test]
    fn the_default_function_timeout_is_thirty_seconds() {
        assert_eq!(DEFAULT_FUNCTION_TIMEOUT, Duration::from_secs(30));
    }

    // --- registration flags ------------------------------------------------

    #[test]
    fn deterministic_opts_into_the_sqlite_flag() {
        assert_eq!(
            function_flags(true).bits(),
            (FunctionFlags::SQLITE_UTF8 | FunctionFlags::SQLITE_DETERMINISTIC).bits()
        );
        assert_eq!(
            function_flags(false).bits(),
            FunctionFlags::SQLITE_UTF8.bits()
        );
    }

    // --- worker state machine (scripted transport double) -------------------

    struct FakeTransport {
        sent: Vec<String>,
        responses: Vec<Result<String, TransportError>>,
        fail_send: bool,
    }

    impl Transport for FakeTransport {
        fn send_line(&mut self, line: &str) -> Result<(), TransportError> {
            if self.fail_send {
                return Err(TransportError::Closed);
            }
            self.sent.push(line.to_string());
            Ok(())
        }

        fn recv_line(&mut self, _timeout: Duration) -> Result<String, TransportError> {
            self.responses.remove(0)
        }
    }

    /// A worker over a spawner that scripts each spawn's responses and counts
    /// spawns via the shared cell.
    fn scripted_worker(
        scripts: Vec<Vec<Result<String, TransportError>>>,
        fail_send: bool,
    ) -> (Worker, Arc<Mutex<usize>>) {
        let spawn_count = Arc::new(Mutex::new(0));
        let scripts = Arc::new(Mutex::new(scripts));
        let count = Arc::clone(&spawn_count);
        let worker = Worker::with_spawner(
            "embed",
            "embedder worker",
            Duration::from_secs(5),
            Box::new(move || -> Result<Box<dyn Transport>, String> {
                *count.lock().unwrap() += 1;
                let responses = scripts.lock().unwrap().remove(0);
                Ok(Box::new(FakeTransport {
                    sent: Vec::new(),
                    responses,
                    fail_send,
                }))
            }),
        );
        (worker, spawn_count)
    }

    #[test]
    fn call_spawns_once_and_reuses_the_worker() {
        let (worker, spawns) = scripted_worker(
            vec![vec![
                Ok(r#"{"ok": "A"}"#.to_string()),
                Ok(r#"{"ok": "B"}"#.to_string()),
            ]],
            false,
        );
        assert_eq!(worker.call(&[text("a")]).unwrap(), text("A"));
        assert_eq!(worker.call(&[text("b")]).unwrap(), text("B"));
        assert_eq!(*spawns.lock().unwrap(), 1);
    }

    #[test]
    fn constructing_a_worker_spawns_nothing() {
        let (worker, spawns) = scripted_worker(vec![], false);
        drop(worker);
        assert_eq!(*spawns.lock().unwrap(), 0);
    }

    #[test]
    fn spawn_failure_names_the_function_and_command() {
        let worker = Worker::with_spawner(
            "embed",
            "missing-binary worker",
            Duration::from_secs(5),
            Box::new(|| Err("no such file".to_string())),
        );
        let err = worker.call(&[]).unwrap_err();
        assert!(err.contains("failed to start worker"), "got: {err}");
        assert!(err.contains("`embed`"), "got: {err}");
        assert!(err.contains("missing-binary worker"), "got: {err}");
        assert!(err.contains("no such file"), "got: {err}");
    }

    #[test]
    fn protocol_err_fails_the_call_but_keeps_the_worker() {
        let (worker, spawns) = scripted_worker(
            vec![vec![
                Ok(r#"{"err": "boom"}"#.to_string()),
                Ok(r#"{"ok": 1}"#.to_string()),
            ]],
            false,
        );
        assert_eq!(worker.call(&[]).unwrap_err(), "boom");
        assert_eq!(worker.call(&[]).unwrap(), Value::Integer(1));
        assert_eq!(*spawns.lock().unwrap(), 1, "err response must not respawn");
    }

    #[test]
    fn timeout_drops_the_worker_and_names_the_remedy() {
        let (worker, spawns) = scripted_worker(
            vec![
                vec![Err(TransportError::Timeout)],
                vec![Ok(r#"{"ok": 1}"#.to_string())],
            ],
            false,
        );
        let err = worker.call(&[]).unwrap_err();
        assert!(err.contains("timed out after 5s"), "got: {err}");
        assert!(err.contains("`embed`"), "got: {err}");
        assert!(err.contains("`timeout`"), "got: {err}");
        // The next call starts a fresh worker.
        assert_eq!(worker.call(&[]).unwrap(), Value::Integer(1));
        assert_eq!(*spawns.lock().unwrap(), 2);
    }

    #[test]
    fn a_closed_worker_drops_the_transport_with_an_actionable_error() {
        let (worker, spawns) = scripted_worker(
            vec![
                vec![Err(TransportError::Closed)],
                vec![Ok(r#"{"ok": 1}"#.to_string())],
            ],
            false,
        );
        let err = worker.call(&[]).unwrap_err();
        assert!(err.contains("exited before replying"), "got: {err}");
        assert!(err.contains("stderr"), "got: {err}");
        assert_eq!(worker.call(&[]).unwrap(), Value::Integer(1));
        assert_eq!(*spawns.lock().unwrap(), 2);
    }

    #[test]
    fn a_send_failure_reads_as_a_dead_worker() {
        let (worker, _) = scripted_worker(vec![vec![]], true);
        let err = worker.call(&[]).unwrap_err();
        assert!(err.contains("not accepting requests"), "got: {err}");
        assert!(err.contains("`embed`"), "got: {err}");
    }

    #[test]
    fn an_invalid_response_line_is_reported_verbatim() {
        let (worker, _) = scripted_worker(vec![vec![Ok("garbage".to_string())]], false);
        let err = worker.call(&[]).unwrap_err();
        assert!(err.contains("invalid response"), "got: {err}");
        assert!(err.contains("garbage"), "got: {err}");
        assert!(err.contains("`embed`"), "got: {err}");
    }

    #[test]
    fn call_sends_the_encoded_request() {
        // Route the sent line back through the response so the double stays
        // self-contained: echo transport.
        struct EchoTransport;
        impl Transport for EchoTransport {
            fn send_line(&mut self, line: &str) -> Result<(), TransportError> {
                assert_eq!(line, r#"{"call":["x",3]}"#);
                Ok(())
            }
            fn recv_line(&mut self, _timeout: Duration) -> Result<String, TransportError> {
                Ok(r#"{"ok": null}"#.to_string())
            }
        }
        let worker = Worker::with_spawner(
            "f",
            "cmd",
            Duration::from_secs(1),
            Box::new(|| Ok(Box::new(EchoTransport))),
        );
        assert_eq!(
            worker.call(&[text("x"), Value::Integer(3)]).unwrap(),
            Value::Null
        );
    }
}
