**Fixed**

A path-table written with repeated separators after the leading dot (`'.//docs/a.md'`, `'.//'`) resolves under the index root, the same as `'./docs/a.md'`, instead of against the filesystem root.
