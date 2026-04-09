/**
 * Diff old and new file content to produce minimal row events.
 *
 * Mirrors the Rust differ logic:
 * - New file: all inserts
 * - Deleted file: all deletes
 * - Modified file with single row: update event
 * - Modified multi-row file: line-index-based diffing with full-replace
 *   threshold at >50% changed or file shrinkage
 */
function rowsEqual(a, b) {
    const keysA = Object.keys(a);
    const keysB = Object.keys(b);
    if (keysA.length !== keysB.length)
        return false;
    for (const key of keysA) {
        if (a[key] !== b[key])
            return false;
    }
    return true;
}
/** Diff old and new row arrays to produce minimal row events. */
export function diff(table, oldRows, newRows, filePath) {
    if (oldRows === null && newRows === null) {
        return [];
    }
    if (oldRows === null && newRows !== null) {
        return newRows.map((row) => ({
            table,
            action: "insert",
            row,
            oldRow: null,
            error: null,
            filePath,
        }));
    }
    if (oldRows !== null && newRows === null) {
        return oldRows.map((row) => ({
            table,
            action: "delete",
            row,
            oldRow: null,
            error: null,
            filePath,
        }));
    }
    // Both non-null
    return diffRows(table, oldRows, newRows, filePath);
}
function diffRows(table, oldRows, newRows, filePath) {
    // If file shrunk, do full replace
    if (newRows.length < oldRows.length) {
        return fullReplace(table, oldRows, newRows, filePath);
    }
    const overlap = oldRows.length;
    let changed = 0;
    for (let i = 0; i < overlap; i++) {
        if (!rowsEqual(oldRows[i], newRows[i])) {
            changed++;
        }
    }
    // For multi-row files, if more than half of overlapping rows changed, full replace.
    // Single-row files (overlap == 1) never trigger full replace.
    if (overlap > 1 && changed * 2 > overlap) {
        return fullReplace(table, oldRows, newRows, filePath);
    }
    const events = [];
    // Update events for changed lines
    for (let i = 0; i < overlap; i++) {
        if (!rowsEqual(oldRows[i], newRows[i])) {
            events.push({
                table,
                action: "update",
                row: newRows[i],
                oldRow: oldRows[i],
                error: null,
                filePath,
            });
        }
    }
    // Insert events for appended lines
    for (let i = overlap; i < newRows.length; i++) {
        events.push({
            table,
            action: "insert",
            row: newRows[i],
            oldRow: null,
            error: null,
            filePath,
        });
    }
    return events;
}
function fullReplace(table, oldRows, newRows, filePath) {
    const events = [];
    for (const row of oldRows) {
        events.push({
            table,
            action: "delete",
            row,
            oldRow: null,
            error: null,
            filePath,
        });
    }
    for (const row of newRows) {
        events.push({
            table,
            action: "insert",
            row,
            oldRow: null,
            error: null,
            filePath,
        });
    }
    return events;
}
//# sourceMappingURL=differ.js.map