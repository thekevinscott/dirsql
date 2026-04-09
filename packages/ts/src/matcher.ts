/**
 * Maps file paths to table names based on glob patterns.
 * First matching pattern wins. An ignore list filters paths entirely.
 */

import picomatch from "picomatch";

export class TableMatcher {
  private tableMatchers: Array<{
    isMatch: (path: string) => boolean;
    tableName: string;
  }>;
  private ignoreMatchers: Array<(path: string) => boolean>;

  constructor(
    mappings: Array<[glob: string, tableName: string]>,
    ignorePatterns: string[] = [],
  ) {
    this.tableMatchers = mappings.map(([glob, tableName]) => ({
      isMatch: picomatch(glob, { dot: true }),
      tableName,
    }));
    this.ignoreMatchers = ignorePatterns.map((p) =>
      picomatch(p, { dot: true }),
    );
  }

  /** Returns the table name for a file path, or null if no pattern matches. */
  matchFile(path: string): string | null {
    for (const { isMatch, tableName } of this.tableMatchers) {
      if (isMatch(path)) {
        return tableName;
      }
    }
    return null;
  }

  /** Returns true if the path matches any ignore pattern. */
  isIgnored(path: string): boolean {
    return this.ignoreMatchers.some((isMatch) => isMatch(path));
  }
}
