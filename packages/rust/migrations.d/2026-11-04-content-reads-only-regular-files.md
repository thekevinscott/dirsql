### Content reads only regular files

#### Summary

Reading a path-table's `content` now re-opens the file without blocking, and reads it only when the opened file is a regular file. If a FIFO, device or directory replaced the file between the scan and the read, `content` is NULL. Nothing breaks.

#### Required changes

_None._

#### Deprecations removed

_None._

#### Behavior changes without code changes

A file replaced by a FIFO or a device after the scan used to hang the query or read without end. It now yields NULL `content`, and the query returns.

#### Verification

`dirsql "SELECT path, content FROM '*.txt'"` over a directory of text files returns each file's content, as before.
