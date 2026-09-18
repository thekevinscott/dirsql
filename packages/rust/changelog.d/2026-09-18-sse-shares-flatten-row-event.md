**Changed**

The CLI's HTTP `/events` serializer now builds its JSON from
`dirsql::flatten_row_event` instead of its own per-variant `match`, leaving the
core as the single owner of the flattening decisions. The SSE payload is
unchanged byte for byte: same keys, same key order, same null-versus-absent
handling, same blob-to-hex rendering.
