### Core: `CommandError::Spawn` names the program; `CommandError::NotOnPath` added

#### Summary

`command::CommandError` changed shape so a spawn failure can name the program
and say where it was looked for. `Spawn`'s `command` field (the unsplit
template) is now `program` (argv[0]), and a new `NotOnPath` variant replaces
`Spawn` when a name without a `/` is missing from `$PATH`. Only Rust callers
that match on `CommandError` are affected; the Python and TypeScript SDKs and
the CLI see only the error text.

#### Required changes

| Surface | Before | After |
| ------- | ------ | ----- |
| `CommandError::Spawn` | `Spawn { command, source }` | `Spawn { program, source }` |
| Exhaustive `match` on `CommandError` | No `NotOnPath` arm | Add `CommandError::NotOnPath { program, local }`; `local` is `Some(path)` when a file of that name exists in the command's cwd |

#### Deprecations removed

_None._

#### Behavior changes without code changes

- A bare program name missing from `$PATH` returns `CommandError::NotOnPath`
  where it returned `CommandError::Spawn`.
- The error text names the program, not the template:
  ``failed to spawn `extract.py`: not found on $PATH`` rather than
  ``failed to spawn `extract.py {path}`: No such file or directory (os error 2)``.

#### Verification

```bash
cd "$(mktemp -d)"
printf '#!/bin/sh\necho "[]"\n' > extract.sh
chmod +x extract.sh
echo hi > a.md
dirsql query "SELECT * FROM './*.md'" --on-file 'extract.sh {path}'
# expected on stderr: failed to spawn `extract.sh`: not found on $PATH. Names
# without a `/` are resolved against $PATH; use `./extract.sh` to run `<dir>/extract.sh`
```
