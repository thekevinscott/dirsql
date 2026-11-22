import { execFileSync } from "node:child_process";
import { mkdtempSync, readFileSync } from "node:fs";
import { tmpdir } from "node:os";
import path from "node:path";
import { aroundEach, beforeAll, type RunnerTask } from "vitest";

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

const PAIRS = 5;
const peakFile = path.join(mkdtempSync(path.join(tmpdir(), "measure-")), "peak");
const gnuTime = ["/usr/bin/time", "-f", "%M", "-o", peakFile];
const peakMb = () => Number(readFileSync(peakFile, "utf8").trim().split("\n").at(-1)) / 1024;
const mean = (values: number[]) => values.reduce((sum, value) => sum + value, 0) / values.length;
const pairs = new Map<string, number[][]>();

const over = (what: string, got: number, limit: number | undefined, unit: string) =>
  limit !== undefined && got > limit ? [`${what} ${got.toFixed(1)}${unit}, limit ${limit}${unit}`] : [];

// Each repeat is one test run followed by one baseline run.
const repeat = (task: RunnerTask) => {
  if (task.type === "suite") {
    task.tasks.forEach(repeat);
  } else if (task.meta.baseline) {
    task.repeats = PAIRS - 1;
  }
};
beforeAll(({}, file) => file.tasks.forEach(repeat));

const judge = (name: string, seen: number[][], limits: Limits, unit: string, verb: string) => {
  const failures = over(verb, seen.at(-1)![0], limits.max, unit);
  if (seen.length === PAIRS) {
    const [got, base] = [0, 1].map(column => mean(seen.map(pair => pair[column])));
    console.log(`${name}: mean of ${PAIRS} pairs: test ${got.toFixed(1)}${unit}; baseline ${base.toFixed(1)}${unit}; ${(got / base).toFixed(1)}x`);
    failures.push(...over(`mean ${verb}`, got, limits.mean, unit), ...over("mean was", got / base, limits.times_baseline, "x baseline"));
  }
  if (failures.length > 0) throw new Error(failures.join("; "));
};

// Speed and memory are separate passes, as in the pytest side.
aroundEach(async (runTest, { task }) => {
  const { ms: msLimits, mb: mbLimits, baseline, cwd, cli } = task.meta;
  if (!baseline || !msLimits || !mbLimits) {
    return runTest();
  }
  const seen = pairs.get(task.id) ?? [];
  pairs.set(task.id, seen);

  if (process.env.MEASURE === "speed") {
    const start = performance.now();
    await runTest();
    const ms = performance.now() - start;
    const baselineStart = performance.now();
    execFileSync("sh", ["-c", baseline], { cwd, stdio: "ignore" });
    seen.push([ms, performance.now() - baselineStart]);
    return judge(task.name, seen, msLimits, " ms", "took");
  }

  // A CLI test's memory is its child's peak, so the launcher runs under GNU time.
  const launcher = cli && [...cli];
  cli?.unshift(...gnuTime);
  const rss = process.memoryUsage.rss();
  try {
    await runTest();
  } finally {
    if (cli && launcher) cli.splice(0, cli.length, ...launcher);
  }
  const mb = cli ? peakMb() : (process.memoryUsage.rss() - rss) / 2 ** 20;
  execFileSync(gnuTime[0], [...gnuTime.slice(1), "sh", "-c", baseline], { cwd, stdio: "ignore" });
  seen.push([mb, peakMb()]);
  judge(task.name, seen, mbLimits, " MB", "used");
});
