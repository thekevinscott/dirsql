import { readFileSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { describe, expect, it } from "vitest";

const here = dirname(fileURLToPath(import.meta.url));
const repoRoot = resolve(here, "..", "..", "..");
const tomlPath = join(repoRoot, "putitoutthere.toml");

interface PackageBlock {
  name: string;
  body: string;
}

function splitPackageBlocks(toml: string): PackageBlock[] {
  const lines = toml.split("\n");
  const blocks: PackageBlock[] = [];
  let current: string[] | null = null;

  for (const line of lines) {
    if (/^\s*\[\[package\]\]\s*$/.test(line)) {
      if (current) {
        const joined = current.join("\n");
        const nameMatch = joined.match(/^\s*name\s*=\s*"([^"]+)"/m);
        blocks.push({ name: nameMatch?.[1] ?? "(unknown)", body: joined });
      }
      current = [];
      continue;
    }
    if (current !== null) current.push(line);
  }
  if (current) {
    const joined = current.join("\n");
    const nameMatch = joined.match(/^\s*name\s*=\s*"([^"]+)"/m);
    blocks.push({ name: nameMatch?.[1] ?? "(unknown)", body: joined });
  }
  return blocks;
}

describe("putitoutthere.toml", () => {
  it("every package declaring mode = 'bundled-cli' has a [package.bundle_cli] block", () => {
    const toml = readFileSync(tomlPath, "utf8");
    const packages = splitPackageBlocks(toml);

    const missing: string[] = [];
    for (const pkg of packages) {
      const declaresBundledCli = /mode\s*=\s*"bundled-cli"/.test(pkg.body);
      const hasBundleCliBlock = /^\s*\[package\.bundle_cli\]\s*$/m.test(
        pkg.body,
      );
      if (declaresBundledCli && !hasBundleCliBlock) {
        missing.push(pkg.name);
      }
    }

    expect(
      missing,
      `packages declare 'mode = \"bundled-cli\"' but are missing the matching ` +
        `[package.bundle_cli] block (crate_path / bin / features). Without ` +
        `the block, upstream's musl cross-compile + stage + verify steps in ` +
        `_matrix.yml are gated out and the consumer's local 'npm run build' ` +
        `is the only thing producing the binary — yielding a glibc-linked ` +
        `binary on Linux runners that breaks on hosts with glibc < 2.39 ` +
        `(see dirsql#189, putitoutthere#384). Add a [package.bundle_cli] ` +
        `block under each listed package.`,
    ).toEqual([]);
  });
});
