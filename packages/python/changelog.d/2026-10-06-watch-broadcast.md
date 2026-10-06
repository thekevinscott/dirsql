**Fixed**

- **Every `DirSQL.watch()` stream on one instance now receives every event.** Two or more streams (for example one per connected client) previously split the events between them; now each stream gets every event observed after it was created. (#1397)
