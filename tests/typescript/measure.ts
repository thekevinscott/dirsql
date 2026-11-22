import { execSync } from "node:child_process";
import { aroundEach } from "vitest";

declare module "vitest" {
  interface TaskMeta {
    ms?: { max: number; mean?: number; times_baseline?: number };
    mb?: { max: number; mean?: number };
    baseline?: string;
    cwd?: string;
    tookMs?: number;
    grewMb?: number;
    baselineMs?: number;
  }
}

aroundEach(async (runTest, { task }) => {
  const { ms, mb, baseline, cwd } = task.meta;
  const rssBefore = process.memoryUsage.rss();
  const start = performance.now();
  await runTest();
  const tookMs = performance.now() - start;
  const grewMb = (process.memoryUsage.rss() - rssBefore) / 1024 / 1024;
  Object.assign(task.meta, { tookMs, grewMb });
  if (ms && tookMs > ms.max) throw new Error(`took ${tookMs.toFixed(1)} ms, limit ${ms.max} ms`);
  if (mb && grewMb > mb.max) throw new Error(`grew ${grewMb.toFixed(2)} MB, limit ${mb.max} MB`);
  if (baseline) {
    const baselineStart = performance.now();
    execSync(baseline, { cwd, stdio: "ignore" });
    const baselineMs = performance.now() - baselineStart;
    task.meta.baselineMs = baselineMs;
    console.log(`${task.name}: ${tookMs.toFixed(1)} ms, baseline ${baselineMs.toFixed(1)} ms, ${(tookMs / baselineMs).toFixed(1)}x`);
    if (ms?.times_baseline !== undefined && tookMs > baselineMs * ms.times_baseline) {
      throw new Error(`took ${(tookMs / baselineMs).toFixed(1)}x the baseline, limit ${ms.times_baseline}x`);
    }
  }
});
