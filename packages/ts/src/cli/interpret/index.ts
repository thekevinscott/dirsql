// Re-export the `interpret` entry point so callers (the launcher's
// `main`) can `import { interpret } from "./interpret/index.js"`.
//
// Each helper lives in its own module so unit tests can exercise it in
// isolation without spawning a subprocess.

export { interpret } from "./interpret.js";
