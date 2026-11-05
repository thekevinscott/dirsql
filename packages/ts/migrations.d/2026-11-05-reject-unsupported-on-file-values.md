### `onFile` refuses unsupported value types

#### Summary

The TypeScript SDK's `onFile` rows accept only `null`/`undefined`, `boolean`, `number`, `bigint`, `string` and `Buffer`/`Uint8Array`. Any other value type now fails `ready` and every query with an error naming the table, file, column and type, instead of being stored as its `String()` form. Hooks that returned such values must convert them.

#### Required changes

| Before | After |
|---|---|
| `{ meta: { a: 1 } }` | `{ meta: JSON.stringify(m) }` |
| `{ tags: ["a", "b"] }` | `{ tags: JSON.stringify(tags) }` |
| `{ at: new Date() }` | `{ at: d.toISOString() }` |

#### Deprecations removed

_None._

#### Behavior changes without code changes

A hook returning an unsupported type now rejects `ready` and every query with an error containing `column '<name>': unsupported value type <Type>`.

#### Verification

```sh
node -e 'const fs=require("fs"),os=require("os"),path=require("path");const {DirSQL}=require("dirsql");
const d=fs.mkdtempSync(path.join(os.tmpdir(),"m-"));fs.writeFileSync(path.join(d,"a.json"),"{}");
new DirSQL({root:d,tables:[{name:"t",ddl:"CREATE TABLE t (v)",glob:"*.json",onFile:()=>[{v:{a:1}}]}]}).ready.catch(e=>console.log(e.message))'
```

Expected: a message containing `column 'v': unsupported value type Object`.
