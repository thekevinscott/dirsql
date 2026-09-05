/** Loadable-file glob suffix(es) for the current platform. */
export function platformSuffixes(): string[] {
  if (process.platform === "darwin") {
    return [".dylib", ".node"];
  }
  if (process.platform === "win32") {
    return [".dll", ".node"];
  }
  return [".so", ".node"];
}
