//! Structured table-definition types (issue #202).
//!
//! These replace the hand-written `CREATE TABLE` DDL string with a typed,
//! cross-SDK shape. The single renderer is [`Table::to_ddl`](crate::Table::to_ddl)
//! (plus [`Table::index_ddls`](crate::Table::index_ddls) for table-level
//! indexes), so every binding — Python, TypeScript, and the TOML config —
//! produces an identical SQLite schema from the same logical definition.
//!
//! Arbitrary SQL expressions (`CHECK`, expression `DEFAULT`, `GENERATED`) are
//! not modeled; they ride through verbatim via the `{ sql: "..." }` escape
//! hatch ([`DefaultValue::Sql`], [`Expression`], [`GeneratedColumn`]) and SQLite
//! is the validator.

/// The five SQLite storage classes dirsql exposes for a column `type`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ColumnType {
    #[default]
    Text,
    Integer,
    Real,
    Blob,
    Numeric,
}

impl ColumnType {
    /// The SQLite type keyword (`TEXT`, `INTEGER`, ...).
    pub fn as_sql(&self) -> &'static str {
        match self {
            ColumnType::Text => "TEXT",
            ColumnType::Integer => "INTEGER",
            ColumnType::Real => "REAL",
            ColumnType::Blob => "BLOB",
            ColumnType::Numeric => "NUMERIC",
        }
    }

    /// Parse a type string (case-insensitive) into a [`ColumnType`]. Used by the
    /// bindings and the TOML config to validate the user-supplied `type`.
    pub fn parse(s: &str) -> Option<Self> {
        match s.to_ascii_uppercase().as_str() {
            "TEXT" => Some(ColumnType::Text),
            "INTEGER" => Some(ColumnType::Integer),
            "REAL" => Some(ColumnType::Real),
            "BLOB" => Some(ColumnType::Blob),
            "NUMERIC" => Some(ColumnType::Numeric),
            _ => None,
        }
    }
}

/// A column `default`: either a scalar/NULL literal or a wrapped SQL expression.
///
/// The discrimination rule (issue #202): a scalar / null is a literal; an
/// object with an `sql` key is an expression ([`DefaultValue::Sql`]), rendered
/// as `DEFAULT (<sql>)` so SQLite evaluates it per-row.
#[derive(Debug, Clone, PartialEq)]
pub enum DefaultValue {
    Integer(i64),
    Real(f64),
    Text(String),
    Blob(Vec<u8>),
    Null,
    /// A verbatim SQL expression, e.g. `strftime('%s', 'now')`.
    Sql(String),
}

impl DefaultValue {
    /// Render the text that follows the `DEFAULT` keyword.
    pub fn render(&self) -> String {
        match self {
            DefaultValue::Integer(i) => i.to_string(),
            DefaultValue::Real(f) => {
                // Keep a decimal point so SQLite stores it as REAL, not INTEGER.
                if f.fract() == 0.0 && f.is_finite() {
                    format!("{f:.1}")
                } else {
                    f.to_string()
                }
            }
            DefaultValue::Text(s) => quote_string(s),
            DefaultValue::Blob(b) => render_blob_literal(b),
            DefaultValue::Null => "NULL".to_string(),
            DefaultValue::Sql(sql) => format!("({sql})"),
        }
    }
}

/// A wrapped SQL expression used by a column-level `CHECK`.
#[derive(Debug, Clone, PartialEq)]
pub struct Expression {
    pub sql: String,
}

/// Storage mode for a `GENERATED ALWAYS AS (...)` column.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum GeneratedMode {
    #[default]
    Virtual,
    Stored,
}

impl GeneratedMode {
    pub fn as_sql(&self) -> &'static str {
        match self {
            GeneratedMode::Virtual => "VIRTUAL",
            GeneratedMode::Stored => "STORED",
        }
    }

    /// Parse a mode string (case-insensitive); defaults handled by the caller.
    pub fn parse(s: &str) -> Option<Self> {
        match s.to_ascii_lowercase().as_str() {
            "virtual" => Some(GeneratedMode::Virtual),
            "stored" => Some(GeneratedMode::Stored),
            _ => None,
        }
    }
}

/// A `GENERATED ALWAYS AS (<sql>) [STORED|VIRTUAL]` column body.
#[derive(Debug, Clone, PartialEq)]
pub struct GeneratedColumn {
    pub sql: String,
    pub mode: GeneratedMode,
}

/// A single structured column definition. Construct with field literals plus
/// `..Default::default()` for the constraints you don't need.
#[derive(Debug, Clone, Default)]
pub struct Column {
    pub name: String,
    pub ty: ColumnType,
    pub not_null: bool,
    pub primary_key: bool,
    pub unique: bool,
    pub autoincrement: bool,
    pub collate: Option<String>,
    pub default: Option<DefaultValue>,
    pub check: Option<Expression>,
    pub generated: Option<GeneratedColumn>,
}

impl Column {
    /// Shorthand for a bare `name TYPE` column.
    pub fn new(name: impl Into<String>, ty: ColumnType) -> Self {
        Column {
            name: name.into(),
            ty,
            ..Default::default()
        }
    }

    /// Render this column as a SQLite column definition (the part inside the
    /// `CREATE TABLE (...)` parens).
    pub(crate) fn render(&self) -> String {
        let mut out = format!("{} {}", self.name, self.ty.as_sql());

        // A generated column is mutually exclusive with PRIMARY KEY / DEFAULT;
        // render it and the constraints SQLite allows alongside it.
        if let Some(generated) = &self.generated {
            if self.not_null {
                out.push_str(" NOT NULL");
            }
            if self.unique {
                out.push_str(" UNIQUE");
            }
            out.push_str(&format!(
                " GENERATED ALWAYS AS ({}) {}",
                generated.sql,
                generated.mode.as_sql()
            ));
            if let Some(check) = &self.check {
                out.push_str(&format!(" CHECK ({})", check.sql));
            }
            return out;
        }

        if self.primary_key {
            out.push_str(" PRIMARY KEY");
            if self.autoincrement {
                out.push_str(" AUTOINCREMENT");
            }
        }
        if self.not_null {
            out.push_str(" NOT NULL");
        }
        if self.unique {
            out.push_str(" UNIQUE");
        }
        if let Some(collate) = &self.collate {
            out.push_str(&format!(" COLLATE {collate}"));
        }
        if let Some(default) = &self.default {
            out.push_str(&format!(" DEFAULT {}", default.render()));
        }
        if let Some(check) = &self.check {
            out.push_str(&format!(" CHECK ({})", check.sql));
        }
        out
    }
}

/// A table-level index (`CREATE [UNIQUE] INDEX ... ON <table> (...)`).
#[derive(Debug, Clone, Default)]
pub struct Index {
    pub name: Option<String>,
    pub columns: Vec<String>,
    pub unique: bool,
}

impl Index {
    /// Render the `CREATE INDEX` statement for `table`. When `name` is unset, a
    /// deterministic `dirsql_idx_<table>_<cols>` name is synthesized.
    pub(crate) fn render(&self, table: &str) -> String {
        let name = self
            .name
            .clone()
            .unwrap_or_else(|| format!("dirsql_idx_{}_{}", table, self.columns.join("_")));
        let unique = if self.unique { "UNIQUE " } else { "" };
        format!(
            "CREATE {}INDEX {} ON {} ({})",
            unique,
            name,
            table,
            self.columns.join(", ")
        )
    }
}

/// SQLite single-quote string literal with internal quotes doubled.
fn quote_string(s: &str) -> String {
    format!("'{}'", s.replace('\'', "''"))
}

/// SQLite blob literal: `X'deadbeef'`.
fn render_blob_literal(bytes: &[u8]) -> String {
    use std::fmt::Write;
    let mut hex = String::with_capacity(bytes.len() * 2 + 3);
    hex.push_str("X'");
    for b in bytes {
        let _ = write!(hex, "{b:02x}");
    }
    hex.push('\'');
    hex
}

/// Render the full `CREATE TABLE` body for a structured definition.
///
/// `columns` are the user columns; `tracking` are appended verbatim after them
/// and after any table-level constraints, mirroring the old string-surgery
/// injection but driven from the structured shape.
pub(crate) fn render_create_table(
    name: &str,
    columns: &[Column],
    primary_key: &[String],
    unique: &[Vec<String>],
    without_rowid: bool,
    strict_types: bool,
) -> String {
    let mut parts: Vec<String> = columns.iter().map(Column::render).collect();

    // Tracking columns are column-defs and must precede any table-level
    // constraints (SQLite requires all column-defs before table-constraints).
    parts.push("_dirsql_file_path TEXT NOT NULL".to_string());
    parts.push("_dirsql_row_index INTEGER NOT NULL".to_string());

    if !primary_key.is_empty() {
        parts.push(format!("PRIMARY KEY ({})", primary_key.join(", ")));
    }
    for uq in unique {
        parts.push(format!("UNIQUE ({})", uq.join(", ")));
    }

    let mut options: Vec<&str> = Vec::new();
    if without_rowid {
        options.push("WITHOUT ROWID");
    }
    if strict_types {
        options.push("STRICT");
    }
    let suffix = if options.is_empty() {
        String::new()
    } else {
        format!(" {}", options.join(", "))
    };

    format!("CREATE TABLE {} ({}){}", name, parts.join(", "), suffix)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn column_type_roundtrips() {
        for (s, ty) in [
            ("TEXT", ColumnType::Text),
            ("integer", ColumnType::Integer),
            ("Real", ColumnType::Real),
            ("BLOB", ColumnType::Blob),
            ("numeric", ColumnType::Numeric),
        ] {
            assert_eq!(ColumnType::parse(s), Some(ty));
            assert_eq!(ColumnType::parse(ty.as_sql()), Some(ty));
        }
        assert_eq!(ColumnType::parse("VARCHAR"), None);
        assert_eq!(ColumnType::default(), ColumnType::Text);
    }

    #[test]
    fn renders_bare_column() {
        assert_eq!(
            Column::new("title", ColumnType::Text).render(),
            "title TEXT"
        );
    }

    #[test]
    fn renders_every_constraint() {
        let col = Column {
            name: "id".into(),
            ty: ColumnType::Integer,
            primary_key: true,
            autoincrement: true,
            ..Default::default()
        };
        assert_eq!(col.render(), "id INTEGER PRIMARY KEY AUTOINCREMENT");

        let col = Column {
            name: "name".into(),
            ty: ColumnType::Text,
            not_null: true,
            unique: true,
            collate: Some("NOCASE".into()),
            ..Default::default()
        };
        assert_eq!(col.render(), "name TEXT NOT NULL UNIQUE COLLATE NOCASE");
    }

    #[test]
    fn renders_scalar_defaults() {
        assert_eq!(
            Column {
                name: "t".into(),
                ty: ColumnType::Text,
                default: Some(DefaultValue::Text("untitled".into())),
                ..Default::default()
            }
            .render(),
            "t TEXT DEFAULT 'untitled'"
        );
        assert_eq!(DefaultValue::Integer(5).render(), "5");
        assert_eq!(DefaultValue::Real(1.5).render(), "1.5");
        assert_eq!(DefaultValue::Real(2.0).render(), "2.0");
        assert_eq!(DefaultValue::Null.render(), "NULL");
        assert_eq!(DefaultValue::Text("a'b".into()).render(), "'a''b'");
        assert_eq!(DefaultValue::Blob(vec![0xde, 0xad]).render(), "X'dead'");
    }

    #[test]
    fn renders_sql_default_with_parens() {
        let col = Column {
            name: "ts".into(),
            ty: ColumnType::Integer,
            default: Some(DefaultValue::Sql("strftime('%s', 'now')".into())),
            ..Default::default()
        };
        assert_eq!(col.render(), "ts INTEGER DEFAULT (strftime('%s', 'now'))");
    }

    #[test]
    fn renders_check_and_generated() {
        let col = Column {
            name: "body".into(),
            ty: ColumnType::Text,
            check: Some(Expression {
                sql: "length(body) > 0".into(),
            }),
            ..Default::default()
        };
        assert_eq!(col.render(), "body TEXT CHECK (length(body) > 0)");

        let col = Column {
            name: "body_len".into(),
            ty: ColumnType::Integer,
            generated: Some(GeneratedColumn {
                sql: "length(body)".into(),
                mode: GeneratedMode::Stored,
            }),
            ..Default::default()
        };
        assert_eq!(
            col.render(),
            "body_len INTEGER GENERATED ALWAYS AS (length(body)) STORED"
        );
        assert_eq!(GeneratedMode::parse("stored"), Some(GeneratedMode::Stored));
        assert_eq!(
            GeneratedMode::parse("VIRTUAL"),
            Some(GeneratedMode::Virtual)
        );
        assert_eq!(GeneratedMode::parse("nope"), None);
    }

    #[test]
    fn renders_virtual_generated_with_constraints() {
        let col = Column {
            name: "slug".into(),
            ty: ColumnType::Text,
            not_null: true,
            unique: true,
            generated: Some(GeneratedColumn {
                sql: "lower(title)".into(),
                mode: GeneratedMode::Virtual,
            }),
            check: Some(Expression {
                sql: "slug <> ''".into(),
            }),
            ..Default::default()
        };
        assert_eq!(
            col.render(),
            "slug TEXT NOT NULL UNIQUE GENERATED ALWAYS AS (lower(title)) VIRTUAL CHECK (slug <> '')"
        );
        assert_eq!(GeneratedMode::Virtual.as_sql(), "VIRTUAL");
    }

    #[test]
    fn renders_full_create_table_with_tracking() {
        let ddl = render_create_table(
            "docs",
            &[
                Column::new("title", ColumnType::Text),
                Column::new("body", ColumnType::Text),
            ],
            &[],
            &[],
            false,
            false,
        );
        assert_eq!(
            ddl,
            "CREATE TABLE docs (title TEXT, body TEXT, \
             _dirsql_file_path TEXT NOT NULL, _dirsql_row_index INTEGER NOT NULL)"
        );
    }

    #[test]
    fn renders_composite_pk_unique_and_table_options() {
        let ddl = render_create_table(
            "t",
            &[
                Column::new("a", ColumnType::Text),
                Column::new("b", ColumnType::Text),
            ],
            &["a".into(), "b".into()],
            &[vec!["a".into(), "b".into()]],
            true,
            true,
        );
        assert!(ddl.contains("PRIMARY KEY (a, b)"));
        assert!(ddl.contains("UNIQUE (a, b)"));
        assert!(ddl.ends_with(") WITHOUT ROWID, STRICT"));
    }

    #[test]
    fn renders_index_with_and_without_name() {
        let idx = Index {
            name: Some("idx_title".into()),
            columns: vec!["title".into()],
            unique: true,
        };
        assert_eq!(
            idx.render("docs"),
            "CREATE UNIQUE INDEX idx_title ON docs (title)"
        );

        let idx = Index {
            name: None,
            columns: vec!["a".into(), "b".into()],
            unique: false,
        };
        assert_eq!(idx.render("t"), "CREATE INDEX dirsql_idx_t_a_b ON t (a, b)");
    }
}
