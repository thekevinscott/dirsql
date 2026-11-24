**Changed**

- **A path-table query over a folder that exists but cannot be listed now fails with an error naming it.** The query fails with `cannot list folder '<path>': <reason>`, as config tables and parsed path-tables already did. Before, the folder was read as empty and the query returned the other folders' rows with exit status 0. A folder that does not exist still holds nothing.
