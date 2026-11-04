//! Bind parameters for a query: the values SQLite binds to a statement's
//! placeholders, never spliced into its text.

use std::collections::HashMap;

use crate::db::Value;

/// The values a query binds to its placeholders. `Positional` fills `?`
/// placeholders in order; `Named` fills `:name`, `@name` and `$name`
/// placeholders, keyed by the name without its prefix.
#[derive(Debug, Clone, PartialEq)]
pub enum Params {
    Positional(Vec<Value>),
    Named(HashMap<String, Value>),
}

impl Default for Params {
    fn default() -> Self {
        Params::Positional(Vec::new())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_is_an_empty_sequence() {
        assert_eq!(Params::default(), Params::Positional(Vec::new()));
    }
}
