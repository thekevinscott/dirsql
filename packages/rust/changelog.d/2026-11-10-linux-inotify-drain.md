**Changed** On Linux the watcher reads inotify directly. A watched file is
read when its writer closes it or it is renamed into place, never when it is
created, written or has its metadata changed. `Watcher::drain` returns every
event the kernel has queued, in order, so on Linux it holds every write
finished before the call. macOS and Windows are unchanged.
