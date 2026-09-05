import { describe, expect, it } from "vitest";
import { librarySlug } from "./library-slug.js";
import type { Platform } from "./platforms.js";

function platform(libName: string): Platform {
  return {
    triple: "x86_64-unknown-linux-gnu",
    nodePlatform: "linux",
    nodeArch: "x64",
    libName,
    os: ["linux"],
    cpu: ["x64"],
  };
}

describe("librarySlug", () => {
  it("returns the slug after the `@dirsql/lib-` prefix", () => {
    expect(librarySlug(platform("@dirsql/lib-linux-x64-gnu"))).toBe(
      "linux-x64-gnu",
    );
    expect(librarySlug(platform("@dirsql/lib-darwin-arm64"))).toBe(
      "darwin-arm64",
    );
  });

  it("throws when libName does not start with the `@dirsql/lib-` prefix", () => {
    expect(() => librarySlug(platform("@dirsql/cli-linux-x64-gnu"))).toThrow(
      "libName @dirsql/cli-linux-x64-gnu missing @dirsql/lib- prefix",
    );
  });

  it("rejects a libName that only contains the prefix later on", () => {
    expect(() => librarySlug(platform("x@dirsql/lib-linux-x64-gnu"))).toThrow(
      "libName x@dirsql/lib-linux-x64-gnu missing @dirsql/lib- prefix",
    );
  });
});
