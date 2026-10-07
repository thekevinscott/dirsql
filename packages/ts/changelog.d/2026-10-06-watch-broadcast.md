**Fixed**

- Every `DirSQL.watch()` stream on one TypeScript instance now receives every event observed after its creation, instead of splitting events between consumers. (#1410)
