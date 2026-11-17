**Fixed** Refreshing a declared table after a file changes no longer crashes
when a stored row holds text that is not valid UTF-8, such as a column whose
`DEFAULT` or a trigger writes `CAST(x'ff' AS TEXT)`. The refresh fails with
`column "<name>" holds text that is not valid UTF-8`, the same error a query
returns.
