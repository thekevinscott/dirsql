**Changed** Scans and watcher refreshes of trees under about a hundred directories no longer spawn worker threads, so they finish faster; larger trees still walk on every core.
