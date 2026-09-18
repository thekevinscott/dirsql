**Changed**

The native addon now flattens row events through the core's
`flatten_row_event` instead of its own copy of that match. No user-visible
change: `RowEvent` fields and `action` values are identical.
