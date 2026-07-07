// Run `main()` and translate a rejection into a stderr message +
// non-zero exit.

import { main } from "./main.js";

export function runCli(): void {
  main().catch((e: unknown) => {
    process.stderr.write(
      `dirsql: ${e instanceof Error ? e.message : String(e)}\n`,
    );
    process.exit(1);
  });
}
