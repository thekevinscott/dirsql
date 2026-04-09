/**
 * Walk a directory tree and return all file paths paired with their matching table name.
 */
import * as fs from "node:fs";
import * as path from "node:path";
/** Recursively scan a directory, returning [absolutePath, tableName] pairs. */
export function scanDirectory(root, matcher) {
    const results = [];
    walk(root, root, matcher, results);
    return results;
}
function walk(dir, root, matcher, results) {
    let entries;
    try {
        entries = fs.readdirSync(dir, { withFileTypes: true });
    }
    catch {
        return;
    }
    for (const entry of entries) {
        const fullPath = path.join(dir, entry.name);
        const relPath = path.relative(root, fullPath);
        if (matcher.isIgnored(relPath)) {
            continue;
        }
        if (entry.isDirectory()) {
            walk(fullPath, root, matcher, results);
        }
        else if (entry.isFile()) {
            const tableName = matcher.matchFile(relPath);
            if (tableName !== null) {
                results.push([fullPath, tableName]);
            }
        }
    }
}
//# sourceMappingURL=scanner.js.map