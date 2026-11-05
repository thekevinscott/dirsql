**Fixed** An on-file hook line holding a lone surrogate escape (an unpaired
`\uD800`-`\uDFFF`, which Python's `json.dumps` emits for surrogateescape-decoded
text) still fails the table, but the error now names the line and the escape,
`line 1: lone surrogate \ud800 is not valid Unicode`, instead of
`unexpected end of hex escape`.
