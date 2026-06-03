// Single-line NDJSON writer. Each protocol message is one JSON object
// followed by `\n`; `process.stdout.write` flushes to the underlying
// pipe synchronously on most platforms when the stream is a pipe (as
// it is when the orchestrator spawns this process).

export function writeMessage(msg: unknown): void {
  process.stdout.write(`${JSON.stringify(msg)}\n`);
}
