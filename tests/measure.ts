import { aroundEach } from "vitest";

declare module "vitest" {
  interface TaskMeta {
    ms?: { max: number; mean?: number };
    mb?: { max: number; mean?: number };
    tookMs?: number;
    grewMb?: number;
  }
}

aroundEach(async (runTest, { task }) => {
  const { ms, mb } = task.meta;
  const rssBefore = process.memoryUsage.rss();
  const start = performance.now();
  await runTest();
  const tookMs = performance.now() - start;
  const grewMb = (process.memoryUsage.rss() - rssBefore) / 1024 / 1024;
  Object.assign(task.meta, { tookMs, grewMb });
  if (ms && tookMs > ms.max) throw new Error(`took ${tookMs.toFixed(1)} ms, limit ${ms.max} ms`);
  if (mb && grewMb > mb.max) throw new Error(`grew ${grewMb.toFixed(2)} MB, limit ${mb.max} MB`);
});
