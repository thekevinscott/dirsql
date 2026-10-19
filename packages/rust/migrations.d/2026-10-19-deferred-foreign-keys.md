### Foreign keys checked once per unit of work

#### Summary

A `REFERENCES` clause in a table's `ddl` is now checked at the end of the build, or of one watch batch, rather than per row. Builds that failed at random depending on fill order now succeed; nothing breaks.

#### Required changes

_None._

#### Deprecations removed

_None._

#### Behavior changes without code changes

- A build with an orphaned row fails with a message naming the table, the file and the unmatched values.
- In watch mode, a batch that would orphan a row is rolled back whole and reported as one error event; the database keeps its previous state, and the batch is retried with later batches until one resolves the orphan. Before, each file was applied on its own and a later edit of a referenced parent failed.

#### Verification

```bash
dirsql query "SELECT count(*) FROM child" -c ./.dirsql.toml
```

With a child table declaring `REFERENCES parent(id)`, prints the count on every run instead of failing intermittently with `FOREIGN KEY constraint failed`.
