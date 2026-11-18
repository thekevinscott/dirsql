**Fixed**

- **On Windows, replacing a watched file by renaming another over it no longer sometimes reports its rows deleted and then inserted.** Windows reports the replaced file as removed before it reports the rename; when the removal was applied on its own, every row of the file was reported deleted, and a query in between saw the table without them. A file reported removed that is still there is now re-read, on every platform, so the change reports the rows that differ.
