import {
  existsSync,
  lstatSync,
  mkdirSync,
  mkdtempSync,
  readFileSync,
  readlinkSync,
  rmSync,
  symlinkSync,
  writeFileSync,
} from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { afterEach, beforeEach, describe, expect, it } from "vitest";
import { KEEP_DIRS, KEEP_FILES, restoreSymlink, stageDocs } from "./stageDocs.js";

let work: string;
let workspaceDocs: string;
let packageDocs: string;

beforeEach(() => {
  work = mkdtempSync(join(tmpdir(), "stage-docs-"));
  workspaceDocs = join(work, "docs");
  packageDocs = join(work, "pkg", "docs");
  mkdirSync(workspaceDocs, { recursive: true });
  mkdirSync(join(work, "pkg"), { recursive: true });
});

afterEach(() => {
  rmSync(work, { recursive: true, force: true });
});

function seedWorkspaceDocs(): void {
  // Files that should be shipped.
  writeFileSync(join(workspaceDocs, "index.md"), "# Home");
  writeFileSync(join(workspaceDocs, "getting-started.md"), "# Getting Started");
  writeFileSync(join(workspaceDocs, "migrations.md"), "# Migrations");
  // Directories that should be shipped recursively.
  mkdirSync(join(workspaceDocs, "guide"));
  writeFileSync(join(workspaceDocs, "guide", "tables.md"), "# Tables");
  mkdirSync(join(workspaceDocs, "api"));
  writeFileSync(join(workspaceDocs, "api", "index.md"), "# API");
  // VitePress / agent / test tooling that must NOT be shipped.
  writeFileSync(join(workspaceDocs, "AGENTS.md"), "internal");
  writeFileSync(join(workspaceDocs, "package.json"), "{}");
  mkdirSync(join(workspaceDocs, ".vitepress"));
  writeFileSync(
    join(workspaceDocs, ".vitepress", "config.ts"),
    "export default {}",
  );
  mkdirSync(join(workspaceDocs, "tests"));
  writeFileSync(join(workspaceDocs, "tests", "home.spec.ts"), "// e2e");
}

describe("stageDocs", () => {
  describe("when packageDocs does not exist yet", () => {
    it("copies the curated set of docs files and directories", () => {
      seedWorkspaceDocs();
      stageDocs(workspaceDocs, packageDocs);

      // Allow-listed files copied through.
      expect(readFileSync(join(packageDocs, "index.md"), "utf8")).toBe("# Home");
      expect(
        readFileSync(join(packageDocs, "getting-started.md"), "utf8"),
      ).toBe("# Getting Started");
      expect(readFileSync(join(packageDocs, "migrations.md"), "utf8")).toBe(
        "# Migrations",
      );

      // Allow-listed directories copied recursively.
      expect(readFileSync(join(packageDocs, "guide", "tables.md"), "utf8")).toBe(
        "# Tables",
      );
      expect(readFileSync(join(packageDocs, "api", "index.md"), "utf8")).toBe(
        "# API",
      );

      // Build / agent / test tooling NOT copied.
      expect(existsSync(join(packageDocs, "AGENTS.md"))).toBe(false);
      expect(existsSync(join(packageDocs, "package.json"))).toBe(false);
      expect(existsSync(join(packageDocs, ".vitepress"))).toBe(false);
      expect(existsSync(join(packageDocs, "tests"))).toBe(false);
    });
  });

  describe("when packageDocs already exists as a symlink", () => {
    it("removes the symlink and stages real files in its place", () => {
      seedWorkspaceDocs();
      symlinkSync(workspaceDocs, packageDocs);
      stageDocs(workspaceDocs, packageDocs);

      expect(lstatSync(packageDocs).isSymbolicLink()).toBe(false);
      expect(lstatSync(packageDocs).isDirectory()).toBe(true);
      expect(existsSync(join(packageDocs, "index.md"))).toBe(true);
    });
  });

  describe("when packageDocs already exists as a real directory", () => {
    it("removes the existing directory before staging", () => {
      seedWorkspaceDocs();
      mkdirSync(packageDocs, { recursive: true });
      writeFileSync(join(packageDocs, "stale.md"), "leftover");
      stageDocs(workspaceDocs, packageDocs);

      expect(existsSync(join(packageDocs, "stale.md"))).toBe(false);
      expect(existsSync(join(packageDocs, "index.md"))).toBe(true);
    });
  });
});

describe("restoreSymlink", () => {
  describe("when packageDocs is a real directory", () => {
    it("replaces it with a symlink pointing at the supplied target", () => {
      mkdirSync(packageDocs, { recursive: true });
      writeFileSync(join(packageDocs, "leftover.md"), "x");

      restoreSymlink(packageDocs, "../../docs");

      expect(lstatSync(packageDocs).isSymbolicLink()).toBe(true);
      expect(readlinkSync(packageDocs)).toBe("../../docs");
    });
  });

  describe("when packageDocs does not exist", () => {
    it("creates the symlink", () => {
      restoreSymlink(packageDocs, "../../docs");

      expect(lstatSync(packageDocs).isSymbolicLink()).toBe(true);
      expect(readlinkSync(packageDocs)).toBe("../../docs");
    });
  });

  describe("when packageDocs is a broken symlink", () => {
    it("replaces it with a fresh symlink", () => {
      symlinkSync("/nonexistent/path", packageDocs);
      expect(existsSync(packageDocs)).toBe(false); // broken symlink
      restoreSymlink(packageDocs, "../../docs");

      expect(lstatSync(packageDocs).isSymbolicLink()).toBe(true);
      expect(readlinkSync(packageDocs)).toBe("../../docs");
    });
  });
});

describe("KEEP allow-lists", () => {
  it("ship only the markdown content, not the build tooling", () => {
    expect(KEEP_FILES.has("index.md")).toBe(true);
    expect(KEEP_FILES.has("getting-started.md")).toBe(true);
    expect(KEEP_FILES.has("migrations.md")).toBe(true);
    expect(KEEP_FILES.has("AGENTS.md")).toBe(false);
    expect(KEEP_FILES.has("package.json")).toBe(false);

    expect(KEEP_DIRS.has("guide")).toBe(true);
    expect(KEEP_DIRS.has("api")).toBe(true);
    expect(KEEP_DIRS.has(".vitepress")).toBe(false);
    expect(KEEP_DIRS.has("tests")).toBe(false);
  });
});
