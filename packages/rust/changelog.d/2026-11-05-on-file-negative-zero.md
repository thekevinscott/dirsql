**Fixed** A bare `-0` in on-file hook output is now stored as `INTEGER` 0, as
SQLite's JSON functions type it, instead of `REAL` -0.0. `-0.0` and `-0e0` stay
`REAL`.
