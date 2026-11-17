**Fixed** Memory no longer grows after the last `watch()` stream is gone. Once every stream had been dropped, the events of each later file change were kept and never released. A stream now releases its events when it is closed or collected, and while no stream is open none are kept.

**Added** A `watch()` stream can be closed: `async with db.watch() as events:` closes it when the block exits, and `await stream.aclose()` closes it directly.
