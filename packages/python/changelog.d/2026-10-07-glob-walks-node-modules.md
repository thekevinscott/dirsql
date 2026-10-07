**Changed**

- **Globs walk `node_modules`, like bash.** A path-table or config glob such as `./**/*.js` now returns files under `node_modules`. A `.gitignore` that lists it still hides it.
