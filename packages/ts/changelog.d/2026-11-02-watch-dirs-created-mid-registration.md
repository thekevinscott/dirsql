**Fixed** On Linux, a directory created inside a new directory before the watcher registered that new directory is now watched too. Before, files written into it later produced no events.
