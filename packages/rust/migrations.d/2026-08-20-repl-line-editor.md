### A typed REPL statement ends at its semicolon (#988)

#### Summary

The REPL read one line per statement. On a terminal it now reads until the
statement is **terminated**, exactly as `sqlite3` does, so a statement can span
lines. No API changed and no flag was added or removed; what a keystroke means
at the interactive prompt did.

The piped path is untouched: `dirsql < script.sql` and
`echo "SELECT 1" | dirsql` still run one statement per line with no terminator.

#### Required changes

| Before | After | Fix |
| ------ | ----- | --- |
| Typing `SELECT 1` ⏎ at the prompt ran it | The prompt becomes `...>` and waits | Terminate it: `SELECT 1;`. |
| Ctrl-C at the prompt killed the process | Abandons the current line, session continues | Use Ctrl-D, `exit`, or `quit` to leave. |
| No history file was written | `$XDG_DATA_HOME/dirsql/history` (or `~/.local/share/dirsql/history`) is created and appended to | Nothing, unless you want it elsewhere — point `XDG_DATA_HOME` at another directory, or unset both `XDG_DATA_HOME` and `HOME` for an in-memory session. |

#### Deprecations removed

_None._

#### Behavior changes without code changes

- **Statements typed at a terminal now require a `;`.** An unterminated one is
  never executed: reaching EOF mid-statement discards the fragment rather than
  running half of it.
- `exit`, `quit`, and a blank line are exempt from the terminator rule — none
  is SQL, and waiting for a semicolon on the one input meant to end the session
  would trap the user.
- **Ctrl-C no longer terminates the process** at the prompt. It is handled by
  the editor rather than a signal handler, so it clears the line and re-prompts.
  A `SIGINT` delivered from outside (`kill -INT`) is unaffected.
- The REPL writes a history file where it previously wrote nothing. It holds
  the last 1000 statements and is shared across directories, so statements from
  one project are recallable in another.
- On a terminal the prompt is now drawn by the editor rather than written to
  stdout directly. It is still `dirsql> `, with `   ...> ` for a continuation.

#### Verification

```bash
# A statement over several lines runs as one:
dirsql
# dirsql> SELECT basename, size
#    ...> FROM './'
#    ...> LIMIT 1;
# [{"basename":"a.md","size":6}]

# A semicolon inside a literal is data, not a terminator:
# dirsql> SELECT ';' AS s;
# [{"s":";"}]

# Ctrl-C clears the line; the session stays up. Ctrl-D leaves:
echo $?
# 0

# The piped path is unchanged -- no terminator needed:
printf 'SELECT 1 AS n\nSELECT 2 AS n\n' | dirsql
# [{"n":1}]
# [{"n":2}]

# History lands where XDG says:
ls "${XDG_DATA_HOME:-$HOME/.local/share}/dirsql/history"
```
