use dirsql::{DirSQL, Value};
use tempfile::TempDir;

#[test]
fn repeated_result_names_preserve_each_cell_and_projection_order() {
    let root = TempDir::new().unwrap();
    let db = DirSQL::builder().root(root.path()).build().unwrap();

    let result = db
        .query_ordered("SELECT 1 AS a, 2 AS a, 3 AS \"a:1\", 4 AS a")
        .unwrap();

    assert_eq!(result.columns, ["a", "a:2", "a:1", "a:3"]);
    assert_eq!(result.rows[0]["a"], Value::Integer(1));
    assert_eq!(result.rows[0]["a:2"], Value::Integer(2));
    assert_eq!(result.rows[0]["a:1"], Value::Integer(3));
    assert_eq!(result.rows[0]["a:3"], Value::Integer(4));
}
