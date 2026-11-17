### A watched table's `on-file` command receives names that are not valid Unicode as they are on disk

#### Summary

When a watched change re-runs a `[[table]]`'s `on-file` command, a file whose name is not valid Unicode is passed under the name it has on disk, as it already was at build. Nothing breaks; names that are valid Unicode are unchanged.

#### Required changes

_None._

#### Deprecations removed

_None._

#### Behavior changes without code changes

On a filesystem that allows such names, an `on-file` command re-run after a change received `/dir/ok�name.json` for a file named `ok\xffname.json`; it now receives `/dir/ok\xffname.json`. Two files whose names differ only in such a byte are each passed, where one was dropped.

#### Verification

On tmpfs, with a `[[table]]` over `*.json` whose `on-file` command prints `{"found": os.path.isfile(p)}` per argument and one file named `ok\xffname.json`: open the index, write another `.json` file, and `SELECT found FROM <table>` returns `1` for every row.
