**Changed** Under a table with a per-file callback, a file whose name is not
valid UTF-8 is a per-file error: `file name is not valid UTF-8`. The callback
takes its path as a string, and used to be called with a
replacement-character path that does not exist.
