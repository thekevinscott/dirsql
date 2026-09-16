**Fixed**

- **A directory moved out of the watched root now drops the rows its files produced.** Renaming a directory out of the tree (or to a new name within it) reaches the live watcher as one event naming the directory and none for the files beneath it, so their rows lingered until a cold rescan. The watcher now treats a deleted path as a possible directory: every row whose file sat under it is deleted, with a `Delete` event per file, the same as if each file had been removed on its own. The mirror case, a directory moved in, is #1096. (#1109)
