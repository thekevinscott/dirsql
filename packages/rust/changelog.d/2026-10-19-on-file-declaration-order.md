**Changed**

- **Config tables' `on-file` commands run in declaration order.** One table's command finishes before the next table's starts, at startup and when a watched change re-runs several tables; across repeated `-c`, the configs' argv order. Commands of different tables previously overlapped at startup, or ran in the order their files sorted.
