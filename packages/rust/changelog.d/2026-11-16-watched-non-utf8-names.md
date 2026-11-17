**Fixed** After a watched change, a table's `on-file` command receives a
file whose name is not valid Unicode under the name it has on disk. It used
to receive a replacement-character path that does not exist, and two files
whose names differed only in such a byte were passed as one.
