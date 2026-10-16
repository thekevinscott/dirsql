**Changed**

- **Globs over wide directories match on every core without contending.** Each walk thread matches through its own copy of a glob, and a wide directory splits its matching only across the cores the walk leaves idle. Globs such as `./*/*.md` over large trees answer faster. Output is unchanged.
