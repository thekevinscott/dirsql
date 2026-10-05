### Core: a path table's `content` is read ahead when a statement names it

#### Summary

No API changes. A statement that names `content` has the bodies of the rows it
selects read together, several files at a time, before its first row comes
back; one that does not name it reads no file, as before.

#### Required changes

_None._

#### Deprecations removed

_None._

#### Behavior changes without code changes

- A file deleted after a statement's scan found it is still readable to that
  statement if the read-ahead reached it first; it used to yield `NULL`
  whenever the deletion came before the row was emitted. Either way the next
  statement sees the file gone.
- A statement that names `content` holds the text of every row it selects
  for its own duration, where before it held one row's text at a time.
- In a join, the planner scans the path table whose `content` is named and
  probes the other, where before it probed whichever came second.

#### Verification

```bash
cd "$(mktemp -d)"
mkdir -p p1 p2
printf 'one\n' > p1/title.md
printf 'two\n' > p2/title.md
dirsql query "SELECT content FROM './*/title.md' ORDER BY path"
# expected: [{"content":"one\n"},{"content":"two\n"}] -- both bodies, read together
```
