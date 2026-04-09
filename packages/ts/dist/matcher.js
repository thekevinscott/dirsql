/**
 * Maps file paths to table names based on glob patterns.
 * First matching pattern wins. An ignore list filters paths entirely.
 */
import picomatch from "picomatch";
export class TableMatcher {
    tableMatchers;
    ignoreMatchers;
    constructor(mappings, ignorePatterns = []) {
        this.tableMatchers = mappings.map(([glob, tableName]) => ({
            isMatch: picomatch(glob, { dot: true }),
            tableName,
        }));
        this.ignoreMatchers = ignorePatterns.map((p) => picomatch(p, { dot: true }));
    }
    /** Returns the table name for a file path, or null if no pattern matches. */
    matchFile(path) {
        for (const { isMatch, tableName } of this.tableMatchers) {
            if (isMatch(path)) {
                return tableName;
            }
        }
        return null;
    }
    /** Returns true if the path matches any ignore pattern. */
    isIgnored(path) {
        return this.ignoreMatchers.some((isMatch) => isMatch(path));
    }
}
//# sourceMappingURL=matcher.js.map