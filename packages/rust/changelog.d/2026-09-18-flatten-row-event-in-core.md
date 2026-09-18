**Changed**

`RowEvent` flattening now lives once in the core as the `#[doc(hidden)]`
`dirsql::flatten_row_event`, instead of being reimplemented per binding. No
observable behavior change: field values, the four `action` strings, and both
error paths are identical.
