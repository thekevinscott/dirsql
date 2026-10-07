**Changed**

- POSIX classes such as `[[:alpha:]]`, `[[:upper:]]` and `[[:punct:]]` match Unicode characters instead of ASCII only: `./[[:alpha:]].md` now matches `é.md`. `digit`, `xdigit` and `ascii` stay ASCII.
