**Changed**

- **Rows of a wide directory start sooner.** A directory of more than 8192 entries is judged in parts, spread across the cores, and each part's rows go out as soon as it and the parts before it are done. Before, nothing went out until the whole directory had been judged. `./*/*.md` over a tree of wide directories gets faster. Output and its order are unchanged.
