// Coerce an unknown thrown value into a human-readable string for
// stderr / NDJSON `error` fields. `Error.message` when available,
// `String(e)` otherwise.

export function errMessage(e: unknown): string {
  return e instanceof Error ? e.message : String(e);
}
