**Changed**

- A relative path-table accepts `//`, `.` and `..` components and reports the path as written; `./docs//api.md` returns `docs//api.md`, and `./docs/../top.md` matches `top.md` through `docs/..`.
