import { execFileSync } from "node:child_process";
import { appendFileSync, mkdtempSync, readFileSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import path from "node:path";
import { aroundEach } from "vitest";

interface Limits {
  max: number;
  mean?: number;
  times_baseline?: number;
}

declare module "vitest" {
  interface TaskMeta {
    ms?: Limits;
    mb?: Limits;
    baseline?: string;
    cwd?: string;
    cli?: string[];
  }
}

const peakFile = path.join(mkdtempSync(path.join(tmpdir(), "measure-")), "peak");
const gnuTime = ["/usr/bin/time", "-f", "%M", "-o", peakFile];
const peakMb = () => Number(readFileSync(peakFile, "utf8").trim().split("\n").at(-1)) / 1024;
const statusMb = (field: string) =>
  Number(readFileSync("/proc/self/status", "utf8").split("\n").find(line => line.startsWith(field))!.split(/\s+/)[1]) / 1024;

// tests/measure.py sets MEASURE to a results file and runs vitest once per repeat.
aroundEach(async (runTest, { task }) => {
  const { ms, mb, baseline, cwd, cli } = task.meta;
  if (!baseline || !ms || !mb) {
    throw new Error("a measured test needs ms, mb and baseline in its meta");
  }
  // A CLI test's memory is its child's peak, so the launcher runs under GNU time.
  const launcher = cli && [...cli];
  cli?.unshift(...gnuTime);
  // Resets this process's peak, so the peak read after the test belongs to the test.
  writeFileSync("/proc/self/clear_refs", "5");
  const before = statusMb("VmRSS");
  const start = performance.now();
  try {
    await runTest();
  } finally {
    if (cli && launcher) cli.splice(0, cli.length, ...launcher);
  }
  const took = performance.now() - start;
  const used = cli ? peakMb() : statusMb("VmHWM") - before;

  const baselineStart = performance.now();
  execFileSync(gnuTime[0], [...gnuTime.slice(1), "sh", "-c", baseline], { cwd, stdio: "ignore" });
  const line = { file: task.file.filepath, ms: took, mb: used, base_ms: performance.now() - baselineStart, base_mb: peakMb() };
  appendFileSync(process.env.MEASURE!, `${JSON.stringify({ ...line, limits: { ms, mb } })}\n`);
});
