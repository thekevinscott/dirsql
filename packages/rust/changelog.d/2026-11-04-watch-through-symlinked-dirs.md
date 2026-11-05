**Fixed**

- **Watch tracks files reached through a symlinked directory.** A change at a link's target now reaches the rows indexed under the link: an edit updates them, the target leaving the tree deletes them, and its return restores them. A per-table hook is no longer re-run over a path whose target is gone.
