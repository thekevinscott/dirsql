**Fixed**

- **A query never reads a watch batch half applied or rolled back.** A query with fresh reads off (as the TypeScript binding runs them) now waits for an open watch batch to end, so it no longer sees one table updated and another not, or rows a foreign-key failure then rolls back.
