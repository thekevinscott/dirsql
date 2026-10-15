**Changed**

- **Recursive globs with a narrow file name list faster.** For globs such as `./**/file?.md` and `./**/[[:digit:]]*.md`, the walk drops a file whose name cannot match before building its path, skips the `.git` and `.gitignore` lookups in directories whose listing holds neither, and has its threads read the directories the output reaches next. Output and its order are unchanged.
