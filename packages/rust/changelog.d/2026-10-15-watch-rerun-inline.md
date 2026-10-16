**Fixed**

- **A watched table's `on-file` rerun spawns its command sooner.** A rerun over files that fit one invocation runs it directly, without starting extra threads or reading the CPU count, so a change reaches `/events` and `dirsql watch` faster. Output is unchanged.
