import { describe, expect, it } from "vitest";
import { configPathsFromArgv } from "./config-paths-from-argv.js";

describe("configPathsFromArgv", () => {
  it("reads the `--config X` form", () => {
    expect(configPathsFromArgv(["--config", "/cfg/.dirsql.toml"])).toEqual([
      "/cfg/.dirsql.toml",
    ]);
  });

  it("reads the `--config=X` form", () => {
    expect(configPathsFromArgv(["--config=/c/.dirsql.toml"])).toEqual([
      "/c/.dirsql.toml",
    ]);
  });

  it("reads the short `-c X` form", () => {
    expect(configPathsFromArgv(["-c", "/frag/dirsql.toml"])).toEqual([
      "/frag/dirsql.toml",
    ]);
  });

  it("reads the short `-c=X` form", () => {
    expect(configPathsFromArgv(["-c=/frag/dirsql.toml"])).toEqual([
      "/frag/dirsql.toml",
    ]);
  });

  it("reads the short attached `-cX` form", () => {
    expect(configPathsFromArgv(["-c/frag/dirsql.toml"])).toEqual([
      "/frag/dirsql.toml",
    ]);
  });

  it("collects every config flag in argv order", () => {
    expect(
      configPathsFromArgv([
        "-c",
        "a.toml",
        "--config",
        "b.toml",
        "--config=c.toml",
        "-cd.toml",
      ]),
    ).toEqual(["a.toml", "b.toml", "c.toml", "d.toml"]);
  });

  it("reads the config value at any argv position", () => {
    expect(configPathsFromArgv(["-v", "--config", "/x/y", "tail"])).toEqual([
      "/x/y",
    ]);
  });

  it("consumes a config value that looks like a flag", () => {
    // The token after `--config` / `-c` is that flag's value; it is never
    // re-parsed as another config flag.
    expect(configPathsFromArgv(["--config", "-cx.toml"])).toEqual(["-cx.toml"]);
  });

  it("does not mistake other short flags for config", () => {
    expect(configPathsFromArgv(["-v", "-x", "val"])).toEqual([
      "./.dirsql.toml",
    ]);
  });

  it("defaults to ./.dirsql.toml when no config flag is given", () => {
    expect(configPathsFromArgv(["--port", "9000"])).toEqual(["./.dirsql.toml"]);
  });

  it("defaults to ./.dirsql.toml for an empty argv", () => {
    expect(configPathsFromArgv([])).toEqual(["./.dirsql.toml"]);
  });

  it("treats a bare trailing --config as an empty path", () => {
    expect(configPathsFromArgv(["--config"])).toEqual([""]);
  });

  it("treats a bare trailing -c as an empty path", () => {
    expect(configPathsFromArgv(["-c"])).toEqual([""]);
  });

  it("keeps a bare trailing -c after an earlier config", () => {
    expect(configPathsFromArgv(["--config", "a.toml", "-c"])).toEqual([
      "a.toml",
      "",
    ]);
  });
});
