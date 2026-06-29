// The npm `bin` launcher's actual logic: run `main()` and translate a
// rejection into a stderr message + non-zero exit. Split out of the
// `dirsql.ts` entry shim so it carries a colocated unit test instead of
// an exemption (#239).

import { main } from "./main.js";

export function runCli(): void {
  main().catch((e: unknown) => {
    process.stderr.write(
      `dirsql: ${e instanceof Error ? e.message : String(e)}\n`,
    );
    process.exit(1);
  });
}
