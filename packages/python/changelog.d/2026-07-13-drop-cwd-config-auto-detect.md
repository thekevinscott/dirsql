**Changed**

- **The bundled `dirsql` CLI (`uvx dirsql`) no longer auto-loads `./.dirsql.toml`.** With no `-c`/`--config`, `dirsql` serves the baked-in default `files` table instead of a `.dirsql.toml` that happens to sit in the current directory; pass an on-disk config explicitly with `dirsql -c ./.dirsql.toml`. A `-c` naming a missing file is now an error rather than a silent fallback. The core change lives in the shared Rust binary the launcher ships (#602); the Python SDK API is unchanged.
