**Changed**

- **A path table hands its rows to the query while the walk is still running.** `SELECT path FROM './**/*.md'` over a large tree now runs at or under `fd`. Output is unchanged.
