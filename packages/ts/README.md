# `dirsql` (TypeScript SDK)

TypeScript SDK for `dirsql` -- napi-rs bindings wrapping the Rust core (`dirsql`).

## Installation

```bash
pnpm add dirsql
```

Requires a native build step (Rust toolchain). The native module is compiled during `pnpm build`.

## Usage

```typescript
import { DirSQL } from "dirsql";

const db = new DirSQL("/path/to/directory", [
  {
    ddl: "CREATE TABLE users (name TEXT, age INTEGER)",
    glob: "data/*.json",
    extract: (filePath, content) => JSON.parse(content),
  },
]);

const rows = db.query("SELECT * FROM users WHERE age > 25");
console.log(rows);
```

## Building

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
