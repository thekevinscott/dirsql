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
//!   which fails the query with that message. An `{"ok": ...}` response may
//!   carry an optional `"meta"` object alongside it; the one key read today is
//!   `{"meta": {"cached": true}}`, which says the worker answered from its own
//!   cache and feeds the progress line's cache split. `meta` is advisory —
//!   every other key, and a `meta` of a shape this parser does not recognize,
//!   is ignored rather than failing the query.
//! - The worker's stderr is inherited, passing straight through to dirsql's
//!   stderr.
//!
//! Each round-trip is bounded by the function's per-call timeout (its
//! `timeout` key, else the 30-second default — [`DEFAULT_FUNCTION_TIMEOUT`]).
//! A timeout or a worker crash kills the worker and fails the query with an
//! actionable error; the next call starts a fresh worker.
//!
//! ## Batching
//!
//! A function declaring `batch = N` is additionally sent values in bulk:
//!
//! - Request: `{"calls": [[<arg>, ...], ...]}` — up to `N` calls, each
//!   encoded as a single call's argument list.
//! - Response: `{"results": [<response>, ...]}` — one single-call response
//!   (`{"ok": ...}` or `{"err": ...}`) per call, in order — or a top-level
//!   `{"err": "message"}`, which fails every call in the request.
//!
//! The round-trip is bounded by the per-call timeout times the number of
//! calls in the request.
//!
//! Core gathers a statement's values in a *collect pass*: the statement runs
//! once with the function's first real reply standing in for every later
//! distinct argument tuple, which is queued and sent in batches; the
//! statement then runs again served from those replies. A statement that
//! never calls a batched function pays nothing for this — its collect pass
//! is the only run. SQLite names the function's arguments only as it
//! evaluates each row, which is why the values are gathered by running the
//! statement rather than by inspecting it.

use std::collections::{HashMap, HashSet};
use std::io::{BufRead, BufReader, Write};
use std::path::PathBuf;
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::mpsc::{Receiver, RecvTimeoutError, SyncSender};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;
use rusqlite::Connection;
use rusqlite::functions::FunctionFlags;

use crate::db::Value;
use crate::progress::{CallProgress, Progress};

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
    /// The most calls one batched request carries; `None` speaks only the
    /// single-call protocol.
    pub batch: Option<usize>,
    pub cwd: PathBuf,
}

/// Register every resolved function on `conn`, once per accepted arity.
/// Purely registration — no worker is spawned here. Returns the workers that
/// batch, for the query path to run collect passes over.
///
/// Every function shares one `calls` reporter: what a user waiting on a query
/// wants to know is how many round trips it is paying for, not which function
/// made them.
pub(crate) fn register_all(
    conn: &Connection,
    functions: &[ResolvedFunction],
    calls: &Arc<CallReporter>,
) -> rusqlite::Result<Vec<Arc<Worker>>> {
    let mut batched = Vec::new();
    for function in functions {
        let worker = Arc::new(Worker::for_process(function, Arc::clone(calls)));
        register_worker(conn, &worker, &function.args, function.deterministic)?;
        if function.batch.is_some() {
            batched.push(worker);
        }
    }
    Ok(batched)
}

/// Register one worker's function on `conn` under each arity in `arities`.
pub(crate) fn register_worker(
    conn: &Connection,
    worker: &Arc<Worker>,
    arities: &[u8],
    deterministic: bool,
) -> rusqlite::Result<()> {
    for &arity in arities {
        let worker = Arc::clone(worker);
        conn.create_scalar_function(
            &worker.name.clone(),
            i32::from(arity),
            function_flags(deterministic),
            move |ctx| {
                let mut args = Vec::with_capacity(ctx.len());
                for i in 0..ctx.len() {
                    args.push(Value::from(rusqlite::types::Value::from(ctx.get_raw(i))));
                }
                worker
                    .call(&args)
                    .map(|reply| reply.value)
                    .map_err(|message| rusqlite::Error::UserFunctionError(message.into()))
            },
        )?;
    }
    Ok(())
}

/// Counts the worker round trips a query pays for, and reports them.
///
/// A query that calls a declared function on every row spends one round trip
/// per row -- `dirsql-plugin-embeddings` runs `embed(content)` over the whole
/// corpus that way -- so the wall time of a single `query()` can be minutes
/// with nothing on the terminal (dirsql#957).
///
/// The count has to be drawn *and erased* by the process that owns stdout, or
/// the query result lands on top of a leftover line. A worker cannot do that:
/// it is SIGKILLed on teardown, and teardown happens after the result is
/// printed anyway (dirsql#1001).
///
/// Behind a mutex because SQLite invokes a scalar function on whatever thread
/// is running the query.
pub(crate) struct CallReporter {
    state: Mutex<CallState>,
}

struct CallState {
    count: u64,
    /// How many of those round trips the worker flagged as cache hits. Core
    /// cannot derive this: a cache hit *is* a round trip, so only the worker
    /// knows which of its answers cost anything.
    cached: u64,
    progress: Box<dyn CallProgress>,
}

impl CallReporter {
    pub(crate) fn new() -> Arc<Self> {
        Arc::new(Self::with_progress(Box::new(Progress::worker_calls())))
    }

    fn with_progress(progress: Box<dyn CallProgress>) -> Self {
        Self {
            state: Mutex::new(CallState {
                count: 0,
                cached: 0,
                progress,
            }),
        }
    }

    /// Open a phase and return the guard that closes it. A guard rather than
    /// a paired call so an error mid-query still ends the phase, and so the
    /// live line is gone before the caller prints anything.
    pub(crate) fn phase(&self) -> CallPhase<'_> {
        if let Ok(mut state) = self.state.lock() {
            state.count = 0;
            state.cached = 0;
            state.progress.restart();
        }
        CallPhase(self)
    }

    /// Record one round trip, before it is made — a slow worker's first call
    /// is exactly when a user most wants the line to appear. A poisoned mutex
    /// costs the counter, never the call: reporting is not worth failing a
    /// query over.
    fn record(&self) {
        if let Ok(mut state) = self.state.lock() {
            state.count += 1;
            state.report();
        }
    }

    /// Mark a round trip that has come back as one the worker served from its
    /// own cache. Separate from [`record`](Self::record) because cachedness is
    /// only knowable once the response is in hand.
    fn mark_cached(&self) {
        if let Ok(mut state) = self.state.lock() {
            state.cached += 1;
            state.report();
        }
    }

    fn end(&self) {
        if let Ok(mut state) = self.state.lock() {
            let (count, cached) = (state.count, state.cached);
            state.progress.finish(count, cached);
        }
    }
}

impl CallState {
    fn report(&mut self) {
        let (count, cached) = (self.count, self.cached);
        self.progress.update(count, cached);
    }
}

/// The open phase. Ends when it drops.
pub(crate) struct CallPhase<'a>(&'a CallReporter);

impl Drop for CallPhase<'_> {
    fn drop(&mut self) {
        self.0.end();
    }
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
    batch: Option<usize>,
    calls: Arc<CallReporter>,
    inner: Mutex<WorkerInner>,
}

struct WorkerInner {
    spawner: Spawner,
    transport: Option<Box<dyn Transport>>,
    /// The open collect pass, if any.
    collect: Option<Collect>,
    /// Replies already in hand for the statement's real run, keyed by the
    /// request line they answer.
    prefetched: HashMap<String, Result<Reply, String>>,
}

/// One collect pass over a statement.
#[derive(Default)]
struct Collect {
    /// Whether the statement called the function at all.
    invoked: bool,
    /// The first real non-NULL reply, bound for every later call until the
    /// real run. Something the statement can keep computing with — NULL
    /// cannot stand in, since functions downstream (sqlite-vec's distance
    /// functions, say) reject it.
    placeholder: Option<Value>,
    /// Argument tuples awaiting a batched request.
    pending: Vec<Vec<Value>>,
    /// Every request line queued or answered so far, so a repeated argument
    /// tuple costs one call.
    queued: HashSet<String>,
}

impl Worker {
    fn for_process(function: &ResolvedFunction, calls: Arc<CallReporter>) -> Self {
        let command = function.command.clone();
        let cwd = function.cwd.clone();
        Self::with_spawner(
            &function.name,
            &function.command,
            function.timeout,
            function.batch,
            calls,
            Box::new(move || spawn_process(&command, &cwd)),
        )
    }

    fn with_spawner(
        name: &str,
        command: &str,
        timeout: Duration,
        batch: Option<usize>,
        calls: Arc<CallReporter>,
        spawner: Spawner,
    ) -> Self {
        Self {
            name: name.to_string(),
            command: command.to_string(),
            timeout,
            batch,
            calls,
            inner: Mutex::new(WorkerInner {
                spawner,
                transport: None,
                collect: None,
                prefetched: HashMap::new(),
            }),
        }
    }

    /// Answer one call. Outside a collect pass that is one protocol
    /// round-trip, unless the collect pass already fetched the reply. Inside
    /// one, the first call (and any call before a usable placeholder exists)
    /// makes its round-trip; every later distinct argument tuple is queued
    /// for a batched request and answered with the placeholder.
    ///
    /// `Err` carries the message the query fails with. A transport failure
    /// (spawn error, crash, timeout) drops the worker so the next call starts
    /// fresh; a protocol-level `{"err": ...}` leaves the healthy worker
    /// running.
    fn call(&self, args: &[Value]) -> Result<Reply, String> {
        let mut inner = self.lock()?;
        let key = request_line(args);
        if let Some(reply) = inner.prefetched.get(&key) {
            return reply.clone();
        }

        let mut flush_due = false;
        let mut stand_in = None;
        if let Some(collect) = inner.collect.as_mut() {
            collect.invoked = true;
            if let Some(placeholder) = &collect.placeholder {
                stand_in = Some(placeholder.clone());
                if collect.queued.insert(key.clone()) {
                    collect.pending.push(args.to_vec());
                    flush_due = Some(collect.pending.len()) >= self.batch;
                }
            }
        }
        if let Some(value) = stand_in {
            if flush_due {
                self.flush(&mut inner)?;
            }
            return Ok(Reply {
                value,
                cached: false,
            });
        }

        self.calls.record();
        let reply = self
            .exchange(&mut inner, &key, self.timeout, 1)
            .and_then(|line| self.decode(&line));
        if let Ok(Reply { cached: true, .. }) = &reply {
            self.calls.mark_cached();
        }
        if let Some(collect) = inner.collect.as_mut() {
            if let Ok(Reply { value, .. }) = &reply
                && *value != Value::Null
            {
                collect.placeholder = Some(value.clone());
            }
            collect.queued.insert(key.clone());
            inner.prefetched.insert(key, reply.clone());
        }
        reply
    }

    /// Open a collect pass. Anything a previous pass left behind is dropped.
    /// Inert on a worker without `batch`: its calls are real either way.
    pub(crate) fn begin_collect(&self) -> Result<(), String> {
        let mut inner = self.lock()?;
        inner.prefetched.clear();
        if self.batch.is_some() {
            inner.collect = Some(Collect::default());
        }
        Ok(())
    }

    /// Close the collect pass, sending whatever is still queued. Returns
    /// whether the statement called the function at all — the signal for
    /// whether a real run is owed.
    pub(crate) fn end_collect(&self) -> Result<bool, String> {
        let mut inner = self.lock()?;
        let flushed = self.flush(&mut inner);
        let invoked = inner.collect.take().is_some_and(|collect| collect.invoked);
        flushed?;
        Ok(invoked)
    }

    /// Drop the replies gathered for a statement once it has run.
    pub(crate) fn clear(&self) -> Result<(), String> {
        let mut inner = self.lock()?;
        inner.collect = None;
        inner.prefetched.clear();
        Ok(())
    }

    /// How many replies are held for a statement's real run.
    #[cfg(test)]
    pub(crate) fn gathered(&self) -> usize {
        self.inner.lock().unwrap().prefetched.len()
    }

    fn lock(&self) -> Result<std::sync::MutexGuard<'_, WorkerInner>, String> {
        self.inner
            .lock()
            .map_err(|e| format!("function `{}` worker state poisoned: {e}", self.name))
    }

    /// Send the queued argument tuples as one batched request and file each
    /// reply under its call's request line. A worker-level `{"err": ...}` or
    /// a malformed response fails every call in the request; a transport
    /// failure fails the flush itself.
    fn flush(&self, inner: &mut WorkerInner) -> Result<(), String> {
        let pending = match inner.collect.as_mut() {
            Some(collect) if !collect.pending.is_empty() => std::mem::take(&mut collect.pending),
            _ => return Ok(()),
        };
        let count = pending.len();
        let request = batched_request_line(&pending);
        let timeout = self
            .timeout
            .saturating_mul(u32::try_from(count).unwrap_or(u32::MAX));
        let line = self.exchange(inner, &request, timeout, count)?;
        let results = match parse_batched_response(&line, count) {
            Ok(results) => results,
            Err(defect) => {
                let message = format!(
                    "function `{}` worker sent an invalid batched response ({defect}): {line}",
                    self.name
                );
                vec![Response::Err(message); count]
            }
        };
        for (args, response) in pending.iter().zip(results) {
            self.calls.record();
            let reply = match response {
                Response::Ok { value, cached } => {
                    if cached {
                        self.calls.mark_cached();
                    }
                    Ok(Reply { value, cached })
                }
                Response::Err(message) => Err(message),
            };
            inner.prefetched.insert(request_line(args), reply);
        }
        Ok(())
    }

    /// One transport round-trip: spawn the worker if this is the first
    /// request, send the line, wait up to `timeout` for the response.
    /// `count` is how many calls the request carries, for the timeout
    /// message.
    fn exchange(
        &self,
        inner: &mut WorkerInner,
        request: &str,
        timeout: Duration,
        count: usize,
    ) -> Result<String, String> {
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

        if transport.send_line(request).is_err() {
            inner.transport = None;
            return Err(format!(
                "worker for function `{}` (command `{}`) is not accepting requests; \
                 it may have exited — check its stderr above",
                self.name, self.command
            ));
        }

        match transport.recv_line(timeout) {
            Ok(line) => Ok(line),
            Err(TransportError::Timeout) => {
                inner.transport = None;
                let budget = if count == 1 {
                    format!("{timeout:?}")
                } else {
                    format!("{timeout:?} ({count} calls at {:?} each)", self.timeout)
                };
                Err(format!(
                    "call to function `{}` timed out after {budget} (worker command `{}`); \
                     raise the function's `timeout` if the worker legitimately needs \
                     longer per call",
                    self.name, self.command
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
    fn decode(&self, line: &str) -> Result<Reply, String> {
        match parse_response(line) {
            Ok(Response::Ok { value, cached }) => Ok(Reply { value, cached }),
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
    serde_json::json!({ "call": encode_args(args) }).to_string()
}

/// Encode one batched request: `{"calls": [[...], ...]}`, one argument list
/// per call.
fn batched_request_line(calls: &[Vec<Value>]) -> String {
    let encoded: Vec<Vec<serde_json::Value>> = calls.iter().map(|args| encode_args(args)).collect();
    serde_json::json!({ "calls": encoded }).to_string()
}

fn encode_args(args: &[Value]) -> Vec<serde_json::Value> {
    args.iter().map(value_to_json).collect()
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
        Value::Blob(bytes) => serde_json::json!({ "$bytes": STANDARD.encode(bytes) }),
    }
}

/// A completed round trip: the value to bind, plus whatever the response said
/// about how it was produced.
#[derive(Debug, Clone)]
pub(crate) struct Reply {
    value: Value,
    cached: bool,
}

#[derive(Debug, Clone)]
enum Response {
    Ok { value: Value, cached: bool },
    Err(String),
}

/// Parse one response line. `Err` names the defect (invalid JSON, neither
/// `ok` nor `err`, a bad `$bytes` payload) for the caller to wrap.
fn parse_response(line: &str) -> Result<Response, String> {
    let parsed: serde_json::Value =
        serde_json::from_str(line).map_err(|e| format!("invalid JSON: {e}"))?;
    parse_response_value(&parsed)
}

fn parse_response_value(parsed: &serde_json::Value) -> Result<Response, String> {
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
    Ok(Response::Ok {
        value: json_to_sql_value(value)?,
        cached: cached_flag(object),
    })
}

/// Parse one batched response line into one [`Response`] per call, in
/// order. A top-level `{"err": "message"}` is that error for every call.
/// `Err` names the defect (invalid JSON, no `results` array, a count other
/// than `expected`, a malformed entry) for the caller to wrap.
fn parse_batched_response(line: &str, expected: usize) -> Result<Vec<Response>, String> {
    let parsed: serde_json::Value =
        serde_json::from_str(line).map_err(|e| format!("invalid JSON: {e}"))?;
    let object = parsed
        .as_object()
        .ok_or_else(|| "expected a JSON object".to_string())?;
    if let Some(message) = object.get("err") {
        let message = message
            .as_str()
            .ok_or_else(|| "\"err\" must be a string".to_string())?;
        return Ok(vec![Response::Err(message.to_string()); expected]);
    }
    let results = object
        .get("results")
        .and_then(serde_json::Value::as_array)
        .ok_or_else(|| "expected a \"results\" array or an \"err\" key".to_string())?;
    if results.len() != expected {
        return Err(format!(
            "expected {expected} results, got {}",
            results.len()
        ));
    }
    results
        .iter()
        .enumerate()
        .map(|(i, entry)| {
            parse_response_value(entry).map_err(|defect| format!("result {i}: {defect}"))
        })
        .collect()
}

/// Whether the response flagged itself as served from the worker's own cache:
/// `{"meta": {"cached": true}}`. Optional and advisory — it feeds the progress
/// line and nothing else, so an absent, misshapen or non-boolean value reads as
/// "not cached" rather than failing a query over a counter.
fn cached_flag(object: &serde_json::Map<String, serde_json::Value>) -> bool {
    object
        .get("meta")
        .and_then(|meta| meta.get("cached"))
        .and_then(serde_json::Value::as_bool)
        .unwrap_or(false)
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
            let bytes = STANDARD
                .decode(encoded)
                .map_err(|_| "\"$bytes\" is not valid base64".to_string())?;
            Ok(Value::Blob(bytes))
        }
        other => Ok(Value::Text(other.to_string())),
    }
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

    // --- call reporting ----------------------------------------------------

    /// A [`CallProgress`] double recording what it was asked to do, so the
    /// orchestration is assertable in-module. What the reporter *draws* is
    /// `progress.rs`'s business and is unit-tested there.
    #[derive(Clone, Default)]
    struct Recorder(Arc<Mutex<Vec<String>>>);

    impl Recorder {
        fn events(&self) -> Vec<String> {
            self.0.lock().unwrap().clone()
        }

        fn push(&self, event: String) {
            self.0.lock().unwrap().push(event);
        }
    }

    struct RecordingProgress(Recorder);

    impl CallProgress for RecordingProgress {
        fn update(&mut self, done: u64, cached: u64) {
            self.0.push(format!("update {done}/{cached}"));
        }

        fn finish(&mut self, done: u64, cached: u64) {
            self.0.push(format!("finish {done}/{cached}"));
        }

        fn restart(&mut self) {
            self.0.push("restart".to_string());
        }
    }

    fn reporter() -> (CallReporter, Recorder) {
        let recorder = Recorder::default();
        let reporter = CallReporter::with_progress(Box::new(RecordingProgress(recorder.clone())));
        (reporter, recorder)
    }

    #[test]
    fn a_recorded_round_trip_is_reported_as_a_running_count() {
        let (reporter, recorder) = reporter();

        reporter.record();
        reporter.record();

        assert_eq!(recorder.events(), ["update 1/0", "update 2/0"]);
    }

    /// The phase is what the count belongs to: a second query must not carry
    /// the first one's total, and the reporter has to be told to start over.
    #[test]
    fn opening_a_phase_restarts_the_reporter_and_the_count() {
        let (reporter, recorder) = reporter();
        reporter.record();
        reporter.record();

        {
            let _phase = reporter.phase();
            reporter.record();
        }

        assert_eq!(
            recorder.events(),
            [
                "update 1/0",
                "update 2/0",
                "restart",
                "update 1/0",
                "finish 1/0"
            ],
            "the new phase counts from one, not from three"
        );
    }

    /// Dropping the guard is what ends the phase — the whole reason it is a
    /// guard, since a query can leave by an error path.
    #[test]
    fn the_phase_guard_finishes_when_it_drops() {
        let (reporter, recorder) = reporter();

        {
            let _phase = reporter.phase();
            reporter.record();
            reporter.record();
            assert_eq!(
                recorder.events(),
                ["restart", "update 1/0", "update 2/0"],
                "nothing is finished while the phase is open"
            );
        }

        assert_eq!(
            recorder.events(),
            ["restart", "update 1/0", "update 2/0", "finish 2/0"],
            "the closed phase reports the count it accumulated"
        );
    }

    /// A cache hit is a round trip like any other, so it counts twice over:
    /// once in the total and once in the split.
    #[test]
    fn a_cached_round_trip_counts_in_both_the_total_and_the_split() {
        let (reporter, recorder) = reporter();

        reporter.record();
        reporter.mark_cached();

        assert_eq!(recorder.events(), ["update 1/0", "update 1/1"]);
    }

    /// The split counts only what the worker flagged. A run that computes
    /// everything must not read as a run that cached everything.
    #[test]
    fn an_uncached_round_trip_leaves_the_split_at_zero() {
        let (reporter, recorder) = reporter();

        {
            let _phase = reporter.phase();
            reporter.record();
            reporter.record();
            reporter.mark_cached();
        }

        assert_eq!(
            recorder.events(),
            [
                "restart",
                "update 1/0",
                "update 2/0",
                "update 2/1",
                "finish 2/1"
            ],
            "one of the two round trips was flagged, not both"
        );
    }

    /// A phase owns its split as well as its count: a warm second query must
    /// not inherit the first query's cache hits.
    #[test]
    fn opening_a_phase_restarts_the_cache_split_too() {
        let (reporter, recorder) = reporter();
        reporter.record();
        reporter.mark_cached();

        {
            let _phase = reporter.phase();
            reporter.record();
        }

        assert_eq!(
            recorder.events().last().unwrap(),
            "finish 1/0",
            "the new phase starts with no cache hits"
        );
    }

    /// The production constructor wires a real [`Progress`] through the same
    /// trait, so a whole phase runs against it here — the double proves the
    /// orchestration, this proves the wiring is connected to something real.
    /// It draws nothing: the reporter is disabled unless stderr is a terminal.
    #[test]
    fn the_production_reporter_counts_round_trips() {
        let reporter = CallReporter::new();

        {
            let _phase = reporter.phase();
            reporter.record();
            reporter.record();
            reporter.mark_cached();
        }

        let state = reporter.state.lock().unwrap();
        assert_eq!(state.count, 2);
        assert_eq!(state.cached, 1);
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
    fn batched_request_line_encodes_one_argument_list_per_call() {
        let calls = vec![vec![text("a")], vec![text("b"), Value::Integer(1)]];
        assert_eq!(batched_request_line(&calls), r#"{"calls":[["a"],["b",1]]}"#);
    }

    #[test]
    fn value_to_json_encodes_nonfinite_real_as_null() {
        assert!(value_to_json(&Value::Real(f64::NAN)).is_null());
    }

    // --- response decoding -------------------------------------------------

    fn ok_value(line: &str) -> Value {
        ok_response(line).0
    }

    fn ok_cached(line: &str) -> bool {
        ok_response(line).1
    }

    fn ok_response(line: &str) -> (Value, bool) {
        match parse_response(line).unwrap() {
            Response::Ok { value, cached } => (value, cached),
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
            Response::Ok { value, .. } => panic!("expected err, got ok {value:?}"),
        }
    }

    /// The signal core cannot derive for itself: a cache hit and a computed
    /// answer are the same round trip on the wire, so the worker has to say.
    #[test]
    fn a_response_flagged_cached_is_read_as_a_cache_hit() {
        assert!(ok_cached(r#"{"ok": 1, "meta": {"cached": true}}"#));
    }

    #[test]
    fn a_response_without_meta_is_not_a_cache_hit() {
        assert!(!ok_cached(r#"{"ok": 1}"#));
    }

    #[test]
    fn a_response_flagged_uncached_is_not_a_cache_hit() {
        assert!(!ok_cached(r#"{"ok": 1, "meta": {"cached": false}}"#));
    }

    /// `meta` is advisory: it drives a progress counter, so no shape of it is
    /// worth failing a query over. Anything unreadable means "not cached".
    #[test]
    fn an_unreadable_meta_reads_as_uncached_rather_than_failing() {
        assert!(!ok_cached(r#"{"ok": 1, "meta": "not-an-object"}"#));
        assert!(!ok_cached(r#"{"ok": 1, "meta": {}}"#));
        assert!(!ok_cached(r#"{"ok": 1, "meta": {"cached": "yes"}}"#));
        assert!(!ok_cached(r#"{"ok": 1, "meta": null}"#));
        assert!(!ok_cached(r#"{"ok": 1, "meta": [{"cached": true}]}"#));
    }

    /// The value is the `ok` field and nothing else — `meta` never leaks into
    /// what the query binds.
    #[test]
    fn meta_does_not_change_the_decoded_value() {
        assert_eq!(
            ok_value(r#"{"ok": "hi", "meta": {"cached": true}}"#),
            text("hi")
        );
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

    // --- batched response decoding -------------------------------------------

    fn results(line: &str, expected: usize) -> Vec<Result<(Value, bool), String>> {
        parse_batched_response(line, expected)
            .unwrap()
            .into_iter()
            .map(|response| match response {
                Response::Ok { value, cached } => Ok((value, cached)),
                Response::Err(message) => Err(message),
            })
            .collect()
    }

    #[test]
    fn a_batched_response_yields_one_reply_per_call_in_order() {
        assert_eq!(
            results(
                r#"{"results": [{"ok": "A"}, {"err": "bad"}, {"ok": 1, "meta": {"cached": true}}]}"#,
                3
            ),
            [
                Ok((text("A"), false)),
                Err("bad".to_string()),
                Ok((Value::Integer(1), true))
            ]
        );
    }

    /// The worker could not answer the request as a whole; every call in it
    /// gets that message.
    #[test]
    fn a_top_level_err_in_a_batched_response_fails_every_call() {
        assert_eq!(
            results(r#"{"err": "model missing"}"#, 2),
            [
                Err("model missing".to_string()),
                Err("model missing".to_string())
            ]
        );
    }

    #[test]
    fn batched_response_defects_are_named() {
        let defect = |line: &str, expected| parse_batched_response(line, expected).unwrap_err();
        assert!(defect("not json", 1).contains("invalid JSON"));
        assert!(defect("[]", 1).contains("JSON object"));
        assert!(defect(r#"{"ok": 1}"#, 1).contains("\"results\" array"));
        assert!(defect(r#"{"results": 1}"#, 1).contains("\"results\" array"));
        assert!(defect(r#"{"err": 5}"#, 1).contains("must be a string"));
        assert_eq!(
            defect(r#"{"results": [{"ok": 1}]}"#, 2),
            "expected 2 results, got 1"
        );
        assert_eq!(
            defect(r#"{"results": [{"ok": 1}, 7]}"#, 2),
            "result 1: expected a JSON object"
        );
        assert_eq!(
            defect(r#"{"results": [{"ok": {"$bytes": "!!"}}]}"#, 1),
            "result 0: \"$bytes\" is not valid base64"
        );
    }

    #[test]
    fn an_empty_batched_response_matches_an_empty_request() {
        assert!(results(r#"{"results": []}"#, 0).is_empty());
    }

    // --- defaults -----------------------------------------------------------

    #[test]
    fn the_default_function_timeout_is_thirty_seconds() {
        assert_eq!(DEFAULT_FUNCTION_TIMEOUT, Duration::from_secs(30));
    }

    // --- registration --------------------------------------------------------

    fn resolved(name: &str, batch: Option<usize>) -> ResolvedFunction {
        ResolvedFunction {
            name: name.to_string(),
            args: vec![1],
            command: "worker".to_string(),
            deterministic: false,
            timeout: Duration::from_secs(1),
            batch,
            cwd: PathBuf::from("."),
        }
    }

    /// Registration makes each function callable and hands back exactly the
    /// workers that batch, which is what the query path runs collect passes
    /// over.
    #[test]
    fn register_all_registers_every_function_and_returns_the_batching_workers() {
        let conn = Connection::open_in_memory().unwrap();
        let calls = Arc::new(reporter().0);

        let batched = register_all(
            &conn,
            &[resolved("plain", None), resolved("bulk", Some(4))],
            &calls,
        )
        .unwrap();

        let names: Vec<&str> = batched.iter().map(|w| w.name.as_str()).collect();
        assert_eq!(names, ["bulk"]);
        assert!(conn.prepare("SELECT plain(1), bulk(1)").is_ok());
        assert!(
            conn.prepare("SELECT bulk(1, 2)").is_err(),
            "only the listed arity is registered"
        );
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

    pub(super) struct FakeTransport {
        pub(super) sent: Arc<Mutex<Vec<String>>>,
        pub(super) waits: Arc<Mutex<Vec<Duration>>>,
        pub(super) responses: Vec<Result<String, TransportError>>,
        pub(super) fail_send: bool,
    }

    impl Transport for FakeTransport {
        fn send_line(&mut self, line: &str) -> Result<(), TransportError> {
            if self.fail_send {
                return Err(TransportError::Closed);
            }
            self.sent.lock().unwrap().push(line.to_string());
            Ok(())
        }

        fn recv_line(&mut self, timeout: Duration) -> Result<String, TransportError> {
            self.waits.lock().unwrap().push(timeout);
            self.responses.remove(0)
        }
    }

    /// A worker over a spawner that scripts each spawn's responses, plus the
    /// shared cells recording what it did: spawns, request lines sent, and
    /// the timeout each receive waited.
    struct Scripted {
        worker: Worker,
        spawns: Arc<Mutex<usize>>,
        sent: Arc<Mutex<Vec<String>>>,
        waits: Arc<Mutex<Vec<Duration>>>,
        reporter: Arc<CallReporter>,
    }

    impl Scripted {
        fn sent(&self) -> Vec<String> {
            self.sent.lock().unwrap().clone()
        }

        fn spawns(&self) -> usize {
            *self.spawns.lock().unwrap()
        }

        fn counts(&self) -> (u64, u64) {
            let state = self.reporter.state.lock().unwrap();
            (state.count, state.cached)
        }
    }

    fn ok(line: &str) -> Result<String, TransportError> {
        Ok(line.to_string())
    }

    fn scripted_worker(
        scripts: Vec<Vec<Result<String, TransportError>>>,
        fail_send: bool,
    ) -> Scripted {
        scripted(None, scripts, fail_send)
    }

    fn batched_worker(batch: usize, scripts: Vec<Vec<Result<String, TransportError>>>) -> Scripted {
        scripted(Some(batch), scripts, false)
    }

    fn scripted(
        batch: Option<usize>,
        scripts: Vec<Vec<Result<String, TransportError>>>,
        fail_send: bool,
    ) -> Scripted {
        let spawns = Arc::new(Mutex::new(0));
        let sent = Arc::new(Mutex::new(Vec::new()));
        let waits = Arc::new(Mutex::new(Vec::new()));
        let reporter = Arc::new(reporter().0);
        let scripts = Arc::new(Mutex::new(scripts));
        let (count, sent_cell, waits_cell) =
            (Arc::clone(&spawns), Arc::clone(&sent), Arc::clone(&waits));
        let worker = Worker::with_spawner(
            "embed",
            "embedder worker",
            Duration::from_secs(5),
            batch,
            Arc::clone(&reporter),
            Box::new(move || -> Result<Box<dyn Transport>, String> {
                *count.lock().unwrap() += 1;
                let responses = scripts.lock().unwrap().remove(0);
                Ok(Box::new(FakeTransport {
                    sent: Arc::clone(&sent_cell),
                    waits: Arc::clone(&waits_cell),
                    responses,
                    fail_send,
                }))
            }),
        );
        Scripted {
            worker,
            spawns,
            sent,
            waits,
            reporter,
        }
    }

    fn value(worker: &Worker, args: &[Value]) -> Value {
        worker.call(args).unwrap().value
    }

    #[test]
    fn call_spawns_once_and_reuses_the_worker() {
        let s = scripted_worker(
            vec![vec![ok(r#"{"ok": "A"}"#), ok(r#"{"ok": "B"}"#)]],
            false,
        );
        assert_eq!(value(&s.worker, &[text("a")]), text("A"));
        assert_eq!(value(&s.worker, &[text("b")]), text("B"));
        assert_eq!(s.spawns(), 1);
    }

    #[test]
    fn constructing_a_worker_spawns_nothing() {
        let s = scripted_worker(vec![], false);
        let spawns = Arc::clone(&s.spawns);
        drop(s);
        assert_eq!(*spawns.lock().unwrap(), 0);
    }

    #[test]
    fn spawn_failure_names_the_function_and_command() {
        let worker = Worker::with_spawner(
            "embed",
            "missing-binary worker",
            Duration::from_secs(5),
            None,
            Arc::new(reporter().0),
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
        let s = scripted_worker(
            vec![vec![ok(r#"{"err": "boom"}"#), ok(r#"{"ok": 1}"#)]],
            false,
        );
        assert_eq!(s.worker.call(&[]).unwrap_err(), "boom");
        assert_eq!(value(&s.worker, &[]), Value::Integer(1));
        assert_eq!(s.spawns(), 1, "err response must not respawn");
    }

    #[test]
    fn timeout_drops_the_worker_and_names_the_remedy() {
        let s = scripted_worker(
            vec![vec![Err(TransportError::Timeout)], vec![ok(r#"{"ok": 1}"#)]],
            false,
        );
        let err = s.worker.call(&[]).unwrap_err();
        assert!(err.contains("timed out after 5s"), "got: {err}");
        assert!(err.contains("`embed`"), "got: {err}");
        assert!(err.contains("`timeout`"), "got: {err}");
        // The next call starts a fresh worker.
        assert_eq!(value(&s.worker, &[]), Value::Integer(1));
        assert_eq!(s.spawns(), 2);
    }

    #[test]
    fn a_closed_worker_drops_the_transport_with_an_actionable_error() {
        let s = scripted_worker(
            vec![vec![Err(TransportError::Closed)], vec![ok(r#"{"ok": 1}"#)]],
            false,
        );
        let err = s.worker.call(&[]).unwrap_err();
        assert!(err.contains("exited before replying"), "got: {err}");
        assert!(err.contains("stderr"), "got: {err}");
        assert_eq!(value(&s.worker, &[]), Value::Integer(1));
        assert_eq!(s.spawns(), 2);
    }

    #[test]
    fn a_send_failure_reads_as_a_dead_worker() {
        let s = scripted_worker(vec![vec![]], true);
        let err = s.worker.call(&[]).unwrap_err();
        assert!(err.contains("not accepting requests"), "got: {err}");
        assert!(err.contains("`embed`"), "got: {err}");
    }

    #[test]
    fn an_invalid_response_line_is_reported_verbatim() {
        let s = scripted_worker(vec![vec![ok("garbage")]], false);
        let err = s.worker.call(&[]).unwrap_err();
        assert!(err.contains("invalid response"), "got: {err}");
        assert!(err.contains("garbage"), "got: {err}");
        assert!(err.contains("`embed`"), "got: {err}");
    }

    #[test]
    fn call_sends_the_encoded_request_and_waits_the_per_call_timeout() {
        let s = scripted_worker(vec![vec![ok(r#"{"ok": null}"#)]], false);
        assert_eq!(
            value(&s.worker, &[text("x"), Value::Integer(3)]),
            Value::Null
        );
        assert_eq!(s.sent(), [r#"{"call":["x",3]}"#]);
        assert_eq!(*s.waits.lock().unwrap(), [Duration::from_secs(5)]);
    }

    /// Each single call is one round trip in the count; a reply the worker
    /// flagged cached lands in the split.
    #[test]
    fn single_calls_are_counted_with_their_cache_split() {
        let s = scripted_worker(
            vec![vec![
                ok(r#"{"ok": "A"}"#),
                ok(r#"{"ok": "B", "meta": {"cached": true}}"#),
            ]],
            false,
        );
        s.worker.call(&[text("a")]).unwrap();
        s.worker.call(&[text("b")]).unwrap();
        assert_eq!(s.counts(), (2, 1));
    }

    // --- collect pass ---------------------------------------------------------

    /// The shape of the whole mechanism: during the collect pass the first
    /// value makes its own round trip and stands in for every later one,
    /// which are sent together when the pass ends; the real run is then
    /// served without another request.
    #[test]
    fn a_collect_pass_round_trips_the_first_value_and_batches_the_rest() {
        let s = batched_worker(
            8,
            vec![vec![
                ok(r#"{"ok": "A"}"#),
                ok(r#"{"results": [{"ok": "B"}, {"ok": "C"}]}"#),
            ]],
        );

        s.worker.begin_collect().unwrap();
        assert_eq!(value(&s.worker, &[text("a")]), text("A"));
        assert_eq!(value(&s.worker, &[text("b")]), text("A"), "placeholder");
        assert_eq!(value(&s.worker, &[text("c")]), text("A"), "placeholder");
        assert_eq!(
            s.sent(),
            [r#"{"call":["a"]}"#],
            "nothing batched before the pass ends"
        );
        assert!(s.worker.end_collect().unwrap(), "the function was invoked");
        assert_eq!(
            s.sent(),
            [r#"{"call":["a"]}"#, r#"{"calls":[["b"],["c"]]}"#]
        );

        assert_eq!(value(&s.worker, &[text("a")]), text("A"));
        assert_eq!(value(&s.worker, &[text("b")]), text("B"));
        assert_eq!(value(&s.worker, &[text("c")]), text("C"));
        assert_eq!(s.sent().len(), 2, "the real run makes no request");
        assert_eq!(s.spawns(), 1);
    }

    #[test]
    fn a_full_batch_is_sent_as_soon_as_it_fills() {
        let s = batched_worker(
            2,
            vec![vec![
                ok(r#"{"ok": "A"}"#),
                ok(r#"{"results": [{"ok": "B"}, {"ok": "C"}]}"#),
                ok(r#"{"results": [{"ok": "D"}]}"#),
            ]],
        );

        s.worker.begin_collect().unwrap();
        for arg in ["a", "b", "c"] {
            s.worker.call(&[text(arg)]).unwrap();
        }
        assert_eq!(
            s.sent(),
            [r#"{"call":["a"]}"#, r#"{"calls":[["b"],["c"]]}"#],
            "the batch went out when its second value arrived"
        );
        s.worker.call(&[text("d")]).unwrap();
        s.worker.end_collect().unwrap();
        assert_eq!(s.sent().last().unwrap(), r#"{"calls":[["d"]]}"#);
        assert_eq!(value(&s.worker, &[text("d")]), text("D"));
    }

    /// A repeated argument tuple is one call on the wire, not one per row.
    #[test]
    fn a_collect_pass_dedupes_argument_tuples() {
        let s = batched_worker(
            8,
            vec![vec![
                ok(r#"{"ok": "A"}"#),
                ok(r#"{"results": [{"ok": "B"}]}"#),
            ]],
        );

        s.worker.begin_collect().unwrap();
        for arg in ["a", "b", "b", "a"] {
            s.worker.call(&[text(arg)]).unwrap();
        }
        s.worker.end_collect().unwrap();

        assert_eq!(s.sent(), [r#"{"call":["a"]}"#, r#"{"calls":[["b"]]}"#]);
        assert_eq!(value(&s.worker, &[text("b")]), text("B"));
    }

    /// NULL cannot stand in for later values: what the statement does with
    /// the placeholder (sqlite-vec's distance functions, say) would reject it.
    /// Until a real non-NULL reply exists, every call makes its own round trip.
    #[test]
    fn a_null_reply_is_not_used_as_the_placeholder() {
        let s = batched_worker(
            8,
            vec![vec![
                ok(r#"{"ok": null}"#),
                ok(r#"{"ok": "B"}"#),
                ok(r#"{"results": [{"ok": "C"}]}"#),
            ]],
        );

        s.worker.begin_collect().unwrap();
        assert_eq!(value(&s.worker, &[Value::Null]), Value::Null);
        assert_eq!(
            value(&s.worker, &[text("b")]),
            text("B"),
            "its own round trip"
        );
        assert_eq!(
            value(&s.worker, &[text("c")]),
            text("B"),
            "now a placeholder exists"
        );
        s.worker.end_collect().unwrap();

        assert_eq!(
            s.sent(),
            [
                r#"{"call":[null]}"#,
                r#"{"call":["b"]}"#,
                r#"{"calls":[["c"]]}"#
            ]
        );
        assert_eq!(value(&s.worker, &[Value::Null]), Value::Null);
        assert_eq!(value(&s.worker, &[text("c")]), text("C"));
    }

    /// A statement that never reaches the function owes no real run.
    #[test]
    fn an_uninvoked_collect_pass_reports_so_and_sends_nothing() {
        let s = batched_worker(8, vec![]);
        s.worker.begin_collect().unwrap();
        assert!(!s.worker.end_collect().unwrap());
        assert!(s.sent().is_empty());
        assert_eq!(s.spawns(), 0);
    }

    /// Every value is one call in the count whichever request carried it,
    /// and the real run — served from replies already counted — adds none.
    #[test]
    fn batched_results_are_counted_once_with_their_cache_split() {
        let s = batched_worker(
            8,
            vec![vec![
                ok(r#"{"ok": "A"}"#),
                ok(r#"{"results": [{"ok": "B", "meta": {"cached": true}}, {"ok": "C"}]}"#),
            ]],
        );

        s.worker.begin_collect().unwrap();
        for arg in ["a", "b", "c"] {
            s.worker.call(&[text(arg)]).unwrap();
        }
        s.worker.end_collect().unwrap();
        assert_eq!(s.counts(), (3, 1));

        for arg in ["a", "b", "c"] {
            s.worker.call(&[text(arg)]).unwrap();
        }
        assert_eq!(s.counts(), (3, 1), "the real run counts nothing new");
    }

    /// The placeholder is a stand-in, not an answer: it must never be counted
    /// as a round trip or a cache hit.
    #[test]
    fn placeholder_replies_are_not_counted() {
        let s = batched_worker(
            8,
            vec![vec![
                ok(r#"{"ok": "A", "meta": {"cached": true}}"#),
                ok(r#"{"results": [{"ok": "B"}]}"#),
            ]],
        );

        s.worker.begin_collect().unwrap();
        s.worker.call(&[text("a")]).unwrap();
        assert!(!s.worker.call(&[text("b")]).unwrap().cached);
        assert_eq!(s.counts(), (1, 1));
        s.worker.end_collect().unwrap();
        assert_eq!(s.counts(), (2, 1));
    }

    /// A worker-level error on a batched request fails every call it carried,
    /// surfacing where each would have been answered.
    #[test]
    fn a_batched_worker_error_fails_each_call_in_the_request() {
        let s = batched_worker(
            8,
            vec![vec![ok(r#"{"ok": "A"}"#), ok(r#"{"err": "boom"}"#)]],
        );

        s.worker.begin_collect().unwrap();
        for arg in ["a", "b", "c"] {
            s.worker.call(&[text(arg)]).unwrap();
        }
        assert!(s.worker.end_collect().unwrap());

        assert_eq!(value(&s.worker, &[text("a")]), text("A"));
        assert_eq!(s.worker.call(&[text("b")]).unwrap_err(), "boom");
        assert_eq!(s.worker.call(&[text("c")]).unwrap_err(), "boom");
        assert_eq!(s.spawns(), 1, "a protocol-level error keeps the worker");
    }

    #[test]
    fn a_per_entry_error_fails_only_that_call() {
        let s = batched_worker(
            8,
            vec![vec![
                ok(r#"{"ok": "A"}"#),
                ok(r#"{"results": [{"err": "bad b"}, {"ok": "C"}]}"#),
            ]],
        );

        s.worker.begin_collect().unwrap();
        for arg in ["a", "b", "c"] {
            s.worker.call(&[text(arg)]).unwrap();
        }
        s.worker.end_collect().unwrap();

        assert_eq!(s.worker.call(&[text("b")]).unwrap_err(), "bad b");
        assert_eq!(value(&s.worker, &[text("c")]), text("C"));
    }

    #[test]
    fn an_invalid_batched_response_fails_each_call_naming_the_defect() {
        let s = batched_worker(
            8,
            vec![vec![
                ok(r#"{"ok": "A"}"#),
                ok(r#"{"results": [{"ok": "B"}]}"#),
            ]],
        );

        s.worker.begin_collect().unwrap();
        for arg in ["a", "b", "c"] {
            s.worker.call(&[text(arg)]).unwrap();
        }
        s.worker.end_collect().unwrap();

        let err = s.worker.call(&[text("b")]).unwrap_err();
        assert!(err.contains("invalid batched response"), "got: {err}");
        assert!(err.contains("expected 2 results, got 1"), "got: {err}");
        assert!(err.contains("`embed`"), "got: {err}");
        assert_eq!(s.worker.call(&[text("c")]).unwrap_err(), err);
    }

    /// A batched request waits the per-call timeout once per call it carries.
    #[test]
    fn a_batched_request_waits_the_per_call_timeout_per_call() {
        let s = batched_worker(
            8,
            vec![vec![
                ok(r#"{"ok": "A"}"#),
                ok(r#"{"results": [{"ok": "B"}, {"ok": "C"}, {"ok": "D"}]}"#),
            ]],
        );

        s.worker.begin_collect().unwrap();
        for arg in ["a", "b", "c", "d"] {
            s.worker.call(&[text(arg)]).unwrap();
        }
        s.worker.end_collect().unwrap();

        assert_eq!(
            *s.waits.lock().unwrap(),
            [Duration::from_secs(5), Duration::from_secs(15)]
        );
    }

    /// A transport failure on the batched request fails the pass itself; the
    /// worker is dropped so the real run starts a fresh one.
    #[test]
    fn a_batched_timeout_fails_the_pass_and_names_the_budget() {
        let s = batched_worker(
            8,
            vec![
                vec![ok(r#"{"ok": "A"}"#), Err(TransportError::Timeout)],
                vec![ok(r#"{"ok": "B"}"#)],
            ],
        );

        s.worker.begin_collect().unwrap();
        for arg in ["a", "b", "c"] {
            s.worker.call(&[text(arg)]).unwrap();
        }
        let err = s.worker.end_collect().unwrap_err();
        assert!(
            err.contains("timed out after 10s (2 calls at 5s each)"),
            "got: {err}"
        );
        assert!(err.contains("`timeout`"), "got: {err}");

        assert_eq!(value(&s.worker, &[text("b")]), text("B"));
        assert_eq!(s.spawns(), 2);
    }

    #[test]
    fn a_batched_close_fails_the_pass_with_the_dead_worker_error() {
        let s = batched_worker(
            8,
            vec![vec![ok(r#"{"ok": "A"}"#), Err(TransportError::Closed)]],
        );

        s.worker.begin_collect().unwrap();
        s.worker.call(&[text("a")]).unwrap();
        s.worker.call(&[text("b")]).unwrap();
        let err = s.worker.end_collect().unwrap_err();
        assert!(err.contains("exited before replying"), "got: {err}");
    }

    /// A single-call failure during the pass is filed like any reply: the
    /// real run fails the same way without asking again.
    #[test]
    fn a_failed_first_call_is_replayed_on_the_real_run() {
        let s = batched_worker(8, vec![vec![ok(r#"{"err": "boom"}"#)]]);

        s.worker.begin_collect().unwrap();
        assert_eq!(s.worker.call(&[text("a")]).unwrap_err(), "boom");
        assert!(s.worker.end_collect().unwrap());
        assert_eq!(s.worker.call(&[text("a")]).unwrap_err(), "boom");
        assert_eq!(s.sent().len(), 1);
    }

    /// Gathered replies belong to one statement. Once cleared, the next call
    /// goes to the worker again.
    #[test]
    fn clear_drops_the_gathered_replies() {
        let s = batched_worker(
            8,
            vec![vec![
                ok(r#"{"ok": "A"}"#),
                ok(r#"{"results": [{"ok": "B"}]}"#),
                ok(r#"{"ok": "B2"}"#),
            ]],
        );

        s.worker.begin_collect().unwrap();
        s.worker.call(&[text("a")]).unwrap();
        s.worker.call(&[text("b")]).unwrap();
        s.worker.end_collect().unwrap();
        s.worker.clear().unwrap();

        assert_eq!(value(&s.worker, &[text("b")]), text("B2"));
        assert_eq!(s.sent().last().unwrap(), r#"{"call":["b"]}"#);
    }

    #[test]
    fn begin_collect_drops_what_an_earlier_pass_left() {
        let s = batched_worker(
            8,
            vec![vec![
                ok(r#"{"ok": "A"}"#),
                ok(r#"{"results": [{"ok": "B"}]}"#),
                ok(r#"{"ok": "A2"}"#),
            ]],
        );

        s.worker.begin_collect().unwrap();
        s.worker.call(&[text("a")]).unwrap();
        s.worker.call(&[text("b")]).unwrap();
        s.worker.end_collect().unwrap();

        s.worker.begin_collect().unwrap();
        assert_eq!(value(&s.worker, &[text("a")]), text("A2"));
    }

    /// Without `batch` the collect machinery is inert: every call is its own
    /// round trip whether or not a pass is open.
    #[test]
    fn a_worker_without_batch_round_trips_every_call_during_a_pass() {
        let s = scripted_worker(
            vec![vec![ok(r#"{"ok": "A"}"#), ok(r#"{"ok": "B"}"#)]],
            false,
        );

        s.worker.begin_collect().unwrap();
        assert_eq!(value(&s.worker, &[text("a")]), text("A"));
        assert_eq!(value(&s.worker, &[text("b")]), text("B"));
        assert!(!s.worker.end_collect().unwrap(), "no real run is owed");
        assert_eq!(s.sent(), [r#"{"call":["a"]}"#, r#"{"call":["b"]}"#]);
    }
}

/// Scripted workers for tests in other modules.
#[cfg(test)]
pub(crate) mod test_support {
    use super::tests;
    use super::*;

    /// A batching worker for `name` whose spawned transport answers with
    /// `responses` in order. Returns the worker and the cell its request
    /// lines are recorded in.
    pub(crate) fn batched_worker(
        name: &str,
        batch: usize,
        calls: Arc<CallReporter>,
        responses: Vec<&str>,
    ) -> (Arc<Worker>, Arc<Mutex<Vec<String>>>) {
        let sent = Arc::new(Mutex::new(Vec::new()));
        let sent_cell = Arc::clone(&sent);
        let responses: Vec<String> = responses.into_iter().map(str::to_string).collect();
        let worker = Worker::with_spawner(
            name,
            "scripted worker",
            Duration::from_secs(5),
            Some(batch),
            calls,
            Box::new(move || -> Result<Box<dyn Transport>, String> {
                Ok(Box::new(tests::FakeTransport {
                    sent: Arc::clone(&sent_cell),
                    waits: Arc::new(Mutex::new(Vec::new())),
                    responses: responses.iter().cloned().map(Ok).collect(),
                    fail_send: false,
                }))
            }),
        );
        (Arc::new(worker), sent)
    }
}
