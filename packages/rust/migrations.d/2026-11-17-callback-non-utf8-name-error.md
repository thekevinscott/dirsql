### A file whose name is not valid UTF-8 fails under a per-file callback

#### Summary

A table declared with a per-file callback (`Table::new` and its counterparts in the Python and TypeScript SDKs) hands the callback each file's path as a string. A file whose name is not valid UTF-8 has no such string, so it is now a per-file error, at build and after a watched create or modify, in place of a call with a replacement-character path that does not exist. Callback signatures are unchanged. A `[[table]]` with an `on-file` command is unaffected: it receives the name as it is on disk.

#### Required changes

_None._

#### Deprecations removed

_None._

#### Behavior changes without code changes

On a filesystem that allows such names, a file named `ok\xffname.json` matched by a per-file callback table fails the build with ``table `<table>`, file `ok�name.json`: file name is not valid UTF-8``. Created or modified while the index is open, it fails queries with the same error until the file is removed or renamed. Before, the callback was called with `/dir/ok�name.json` and its rows were kept.

#### Verification

On tmpfs, with a per-file callback table over `*.json` and one file named `ok\xffname.json`: opening the index fails with ``table `<table>`, file `ok�name.json`: file name is not valid UTF-8``.
