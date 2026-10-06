**Added**

- **A config `[[table]]` whose glob matches no files now prints a warning to stderr.** `dirsql: table 'sessions': glob 'projects/*/*.jsonl' matched no files under /home/u/.claude` names the table, the glob and the directory it was matched under, so a glob that silently indexes nothing is visible. The exit code is unchanged, and path-tables and programmatic tables do not warn. (#1232)
