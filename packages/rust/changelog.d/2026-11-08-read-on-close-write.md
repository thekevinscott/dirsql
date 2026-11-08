**Changed** On Linux, a watched file is read when its writer closes it or it
is renamed into place, not when it is created or written. A file still being
written no longer produces a half-read row or a parse error event. A new
directory's scan skips a file that fails to parse; its row appears once that
file is closed.
