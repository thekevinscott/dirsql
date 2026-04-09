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
import type { Row } from "./db.js";
export interface InsertEvent {
    table: string;
    action: "insert";
    row: Row;
    oldRow: null;
    error: null;
    filePath: string;
}
export interface UpdateEvent {
    table: string;
    action: "update";
    row: Row;
    oldRow: Row;
    error: null;
    filePath: string;
}
export interface DeleteEvent {
    table: string;
    action: "delete";
    row: Row;
    oldRow: null;
    error: null;
    filePath: string;
}
export interface ErrorEvent {
    table: string;
    action: "error";
    row: null;
    oldRow: null;
    error: string;
    filePath: string;
}
export type RowEvent = InsertEvent | UpdateEvent | DeleteEvent | ErrorEvent;
/** Diff old and new row arrays to produce minimal row events. */
export declare function diff(table: string, oldRows: Row[] | null, newRows: Row[] | null, filePath: string): RowEvent[];
//# sourceMappingURL=differ.d.ts.map