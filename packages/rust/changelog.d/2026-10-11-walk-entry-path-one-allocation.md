**Changed**

- **A tree walk builds each entry's path in one allocation.** Recursive globs over large trees, such as `./**/{a,b}*.md`, list faster because the walk threads no longer contend the allocator. Output is unchanged.
