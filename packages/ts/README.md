# `dirsql` (TypeScript SDK)

TypeScript SDK for `dirsql` -- napi-rs bindings wrapping the Rust core (`dirsql`).

[Documentation](https://thekevinscott.github.io/dirsql/?lang=typescript)

Also available as [`dirsql` on crates.io](https://crates.io/crates/dirsql) and [`dirsql` on PyPI](https://pypi.org/project/dirsql/).

## Installation

```bash
pnpm add dirsql
```

Prebuilt binaries ship for linux-x64, linux-arm64, darwin-x64, darwin-arm64, and win32-x64. npm / pnpm pick up the right one via `optionalDependencies` — no Rust toolchain required.

## Usage

```typescript
import { DirSQL } from "dirsql";

const db = new DirSQL({
  root: "/path/to/directory",
  tables: [
    {
      ddl: "CREATE TABLE users (name TEXT, age INTEGER)",
      glob: "data/*.json",
      extract: (_filePath, content) => JSON.parse(content),
    },
  ],
});

const rows = await db.query("SELECT * FROM users WHERE age > 25");
console.log(rows);
```

## Building (from source)

Building from source requires a Rust toolchain.

```bash
pnpm install
pnpm build
```

## Testing

```bash
pnpm test
```

## License

MIT
