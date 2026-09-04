### dirsql-plugin-embeddings: `ok` responses now carry cache metadata

#### Summary

The worker's success responses gain an optional `"meta"` object:
`{"ok": [floats...], "meta": {"cached": true|false}}`. Nothing is removed and
no signature a user calls changes — the plugin exports no Python API, and its
CLI arguments, exit codes and vectors are all untouched. The one observable
difference is the extra key on the wire, and the progress line dirsql draws
from it.

#### Required changes

_None._

#### Deprecations removed

_None._

#### Behavior changes without code changes

- A worker `ok` line previously read `{"ok": [1.0, 0.0]}` and now reads
  `{"ok": [1.0, 0.0], "meta": {"cached": false}}`. dirsql ignores unknown keys,
  so this is additive for it; a third party parsing worker output with an
  exact-equality check on the whole object will see a mismatch and should
  compare `response["ok"]` instead.
- `{"ok": null}` for a NULL value is unchanged — no compute is attempted, so
  there is no cache state to report.
- With the plugin installed, `dirsql` runs that hit the on-disk cache now show
  the split in the progress line (`ran 41231 worker calls in 2m41s
  (38104 cached)`). A run with no hits looks exactly as it did before.

#### Verification

```bash
printf '{"call": ["hello"]}\n{"call": ["hello"]}\n' \
  | uvx --from dirsql-plugin-embeddings dirsql-plugin-embeddings worker
# expected: first line "cached":false, second line "cached":true
```
