/**
 * Maps file paths to table names based on glob patterns.
 * First matching pattern wins. An ignore list filters paths entirely.
 */
export declare class TableMatcher {
    private tableMatchers;
    private ignoreMatchers;
    constructor(mappings: Array<[glob: string, tableName: string]>, ignorePatterns?: string[]);
    /** Returns the table name for a file path, or null if no pattern matches. */
    matchFile(path: string): string | null;
    /** Returns true if the path matches any ignore pattern. */
    isIgnored(path: string): boolean;
}
//# sourceMappingURL=matcher.d.ts.map