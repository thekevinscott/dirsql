/**
 * Walk a directory tree and return all file paths paired with their matching table name.
 */
import { TableMatcher } from "./matcher.js";
/** Recursively scan a directory, returning [absolutePath, tableName] pairs. */
export declare function scanDirectory(root: string, matcher: TableMatcher): Array<[filePath: string, tableName: string]>;
//# sourceMappingURL=scanner.d.ts.map