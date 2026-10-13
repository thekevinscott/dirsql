**Changed**

- **A path table builds its rows on the walk threads, in smaller rows.** Recursive globs over large trees, such as `./**/*.md`, answer faster and use less memory. Output is unchanged.
