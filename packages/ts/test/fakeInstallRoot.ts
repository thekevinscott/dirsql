// Build a temp dir that looks like an npm install tree with stubbed
// `@dirsql/cli-*` optional-dependency packages. The stub "binaries" are
// tiny shell scripts that echo argv and exit with a configurable code.

import { chmodSync, mkdirSync, mkdtempSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";

export function fakeInstallRoot({
  exitCode = 0,
  echoArgs = true,
} = {}): string {
  const root = mkdtempSync(join(tmpdir(), "dirsql-launcher-"));
  const scope = join(root, "node_modules", "@dirsql");
  mkdirSync(scope, { recursive: true });
  const platforms = [
    "cli-linux-x64-gnu",
    "cli-linux-arm64-gnu",
    "cli-darwin-x64",
    "cli-darwin-arm64",
    "cli-win32-x64-msvc",
  ];
  for (const p of platforms) {
    const pkgDir = join(scope, p);
    mkdirSync(pkgDir, { recursive: true });
    writeFileSync(
      join(pkgDir, "package.json"),
      JSON.stringify({ name: `@dirsql/${p}`, version: "0.0.0-test" }),
    );
    const binName = p.startsWith("cli-win32") ? "dirsql.exe" : "dirsql";
    const script = echoArgs
      ? `#!/bin/sh\necho "args=$*"\nexit ${exitCode}\n`
      : `#!/bin/sh\nexit ${exitCode}\n`;
    const binPath = join(pkgDir, binName);
    writeFileSync(binPath, script);
    chmodSync(binPath, 0o755);
  }
  return root;
}
