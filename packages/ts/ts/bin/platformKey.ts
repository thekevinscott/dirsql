// Return the `platform-arch` key used to look up the per-platform
// optional-dependency package (e.g. `linux-x64`, `darwin-arm64`).

export function platformKey(): string {
  return `${process.platform}-${process.arch}`;
}
