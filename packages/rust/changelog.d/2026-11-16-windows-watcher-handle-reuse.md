**Fixed**

- **On Windows, deleting a directory right after dropping the `DirSQL` built over it no longer hangs.** The watcher could re-arm a read on a directory handle it had already closed; when Windows handed that handle value to the delete, both blocked each other forever. The `notify` dependency moves to 8, which stops a watch before closing its handle.
