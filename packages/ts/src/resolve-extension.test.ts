import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { getCore } from "./core.js";
import { defaultResolver } from "./default-resolver.js";
import type { PackageResolver } from "./package-dir.js";
import { planEntryPath } from "./plan-entry-path.js";
import { resolveExtensionPath } from "./resolve-extension.js";

vi.mock("./core.js");

vi.mock("./default-resolver.js", async () => ({
  ...(await vi.importActual<typeof import("./default-resolver.js")>(
    "./default-resolver.js",
  )),
  defaultResolver: vi.fn(),
}));

vi.mock("./plan-entry-path.js", async () => ({
  ...(await vi.importActual<typeof import("./plan-entry-path.js")>(
    "./plan-entry-path.js",
  )),
  planEntryPath: vi.fn(),
}));

const planExtensionPath = vi.fn();

function fakeResolver(): PackageResolver {
  return { resolve: vi.fn(), paths: vi.fn(() => []) };
}

describe("resolveExtensionPath", () => {
  beforeEach(() => {
    vi.mocked(getCore).mockReturnValue({ planExtensionPath } as never);
  });

  afterEach(() => vi.resetAllMocks());

  it("resolves the entry the core plans, with the injected resolver", () => {
    const entry = { path: "/abs/a.so" };
    planExtensionPath.mockReturnValue(entry);
    vi.mocked(planEntryPath).mockReturnValue("/abs/a.so");
    const resolver = fakeResolver();

    expect(resolveExtensionPath("a.so", "/cfg", false, resolver)).toBe(
      "/abs/a.so",
    );
    expect(planExtensionPath).toHaveBeenCalledWith("a.so", "/cfg", false);
    expect(planEntryPath).toHaveBeenCalledWith(entry, resolver);
    expect(defaultResolver).not.toHaveBeenCalled();
  });

  it("hands defaultResolver() on when none is injected", () => {
    const fallback = fakeResolver();
    planExtensionPath.mockReturnValue({ package: "vec", shadow: "/cfg/vec" });
    vi.mocked(defaultResolver).mockReturnValue(fallback);
    vi.mocked(planEntryPath).mockReturnValue("/nm/vec/vec0.so");

    expect(resolveExtensionPath("vec", "/cfg", true)).toBe("/nm/vec/vec0.so");
    expect(planExtensionPath).toHaveBeenCalledWith("vec", "/cfg", true);
    expect(planEntryPath).toHaveBeenCalledWith(expect.anything(), fallback);
  });
});
