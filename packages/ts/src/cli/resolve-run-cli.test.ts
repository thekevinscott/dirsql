import { afterEach, describe, expect, it, vi } from "vitest";
import { loadNativeCore } from "../load-native-core.js";
import { resolveRunCli } from "./resolve-run-cli.js";

vi.mock("../load-native-core.js");

describe("resolveRunCli", () => {
  afterEach(() => vi.resetAllMocks());

  it("returns the addon's runCli export", () => {
    const runCli = vi.fn(() => 0);
    vi.mocked(loadNativeCore).mockReturnValue({ runCli } as never);

    expect(resolveRunCli()).toBe(runCli);
  });

  it("propagates a load failure rather than masking it", () => {
    vi.mocked(loadNativeCore).mockImplementation(() => {
      throw new Error("no prebuilt addon for linux-x64");
    });

    expect(() => resolveRunCli()).toThrow("no prebuilt addon for linux-x64");
  });

  it("throws when the addon carries no callable runCli", () => {
    // A `@dirsql/lib-*` built without the `cli` feature loads fine but has no
    // CLI; say so rather than throwing a bare TypeError.
    vi.mocked(loadNativeCore).mockReturnValue({} as never);

    // The whole message matters: naming the missing `cli` feature is the
    // half that tells someone how to fix their build.
    expect(() => resolveRunCli()).toThrow(
      "dirsql: the native addon has no callable `runCli` export; " +
        "it was built without the `cli` feature.",
    );
  });

  it("throws when runCli is present but not callable", () => {
    vi.mocked(loadNativeCore).mockReturnValue({ runCli: 7 } as never);

    expect(() => resolveRunCli()).toThrow(
      "dirsql: the native addon has no callable `runCli` export; " +
        "it was built without the `cli` feature.",
    );
  });
});
