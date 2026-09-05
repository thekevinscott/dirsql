/** True when `path` is a bare package name rather than a file path. */
export function isBareName(path: string): boolean {
  // Suffixes that mark a value as "already a file path" (so package resolution is
  // never attempted).
  const loadableSuffixes = [".so", ".dylib", ".dll", ".node"];
  if (path.includes("/") || path.includes("\\")) {
    return false;
  }
  return !loadableSuffixes.some((s) => path.endsWith(s));
}
