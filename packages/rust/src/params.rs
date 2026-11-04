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

/// The value each of a statement's parameters takes, in parameter order,
/// given each parameter's name as SQLite reports it (`None` for a bare `?`).
pub(crate) fn plan<'a>(
    names: &[Option<&str>],
    params: &'a Params,
) -> Result<Vec<&'a Value>, String> {
    match params {
        Params::Positional(values) => {
            if let Some(name) = names.iter().flatten().find(|name| !name.starts_with('?')) {
                return Err(format!(
                    "the query uses the named parameter {name}; pass a mapping, not a sequence"
                ));
            }
            if names.len() != values.len() {
                return Err(format!(
                    "the query has {} parameter(s), but {} value(s) were given",
                    names.len(),
                    values.len()
                ));
            }
            Ok(values.iter().collect())
        }
        Params::Named(values) => {
            let planned = names
                .iter()
                .map(|name| match name {
                    Some(name) if !name.starts_with('?') => values
                        .get(&name[1..])
                        .ok_or_else(|| format!("the named parameter {name} has no value")),
                    _ => Err(format!(
                        "the query uses the positional parameter {}; pass a sequence, not a mapping",
                        name.unwrap_or("?")
                    )),
                })
                .collect::<Result<Vec<_>, String>>()?;
            let mut unused: Vec<&str> = values
                .keys()
                .map(String::as_str)
                .filter(|key| {
                    !names
                        .iter()
                        .flatten()
                        .any(|name| !name.starts_with('?') && name[1..] == **key)
                })
                .collect();
            unused.sort_unstable();
            if let Some(key) = unused.first() {
                return Err(format!(
                    "the parameter {key:?} is not a placeholder in the query"
                ));
            }
            Ok(planned)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn named(pairs: &[(&str, Value)]) -> Params {
        Params::Named(
            pairs
                .iter()
                .map(|(k, v)| ((*k).to_string(), v.clone()))
                .collect(),
        )
    }

    #[test]
    fn default_is_an_empty_sequence() {
        assert_eq!(Params::default(), Params::Positional(Vec::new()));
    }

    #[test]
    fn a_sequence_fills_bare_placeholders_in_order() {
        let params = Params::Positional(vec![Value::Integer(1), Value::Text("a".into())]);
        assert_eq!(
            plan(&[None, None], &params).unwrap(),
            vec![&Value::Integer(1), &Value::Text("a".into())]
        );
    }

    #[test]
    fn a_sequence_fills_numbered_placeholders() {
        let params = Params::Positional(vec![Value::Null]);
        assert_eq!(plan(&[Some("?1")], &params).unwrap(), vec![&Value::Null]);
    }

    #[test]
    fn an_empty_sequence_fits_a_statement_without_placeholders() {
        assert!(plan(&[], &Params::default()).unwrap().is_empty());
    }

    #[test]
    fn a_sequence_too_short_names_both_counts() {
        let err = plan(&[None, None], &Params::Positional(vec![Value::Null])).unwrap_err();
        assert_eq!(
            err,
            "the query has 2 parameter(s), but 1 value(s) were given"
        );
    }

    #[test]
    fn a_sequence_too_long_names_both_counts() {
        let err = plan(&[None], &Params::Positional(vec![Value::Null, Value::Null])).unwrap_err();
        assert_eq!(
            err,
            "the query has 1 parameter(s), but 2 value(s) were given"
        );
    }

    #[test]
    fn a_sequence_for_a_named_placeholder_is_refused() {
        let err = plan(
            &[None, Some(":advice")],
            &Params::Positional(vec![Value::Null; 2]),
        )
        .unwrap_err();
        assert_eq!(
            err,
            "the query uses the named parameter :advice; pass a mapping, not a sequence"
        );
    }

    #[test]
    fn a_mapping_fills_each_prefix_by_name() {
        let params = named(&[
            ("a", Value::Integer(1)),
            ("b", Value::Integer(2)),
            ("c", Value::Integer(3)),
        ]);
        assert_eq!(
            plan(&[Some("$c"), Some(":a"), Some("@b")], &params).unwrap(),
            vec![&Value::Integer(3), &Value::Integer(1), &Value::Integer(2)]
        );
    }

    #[test]
    fn a_mapping_missing_a_name_names_the_placeholder() {
        let err = plan(&[Some(":advice")], &named(&[])).unwrap_err();
        assert_eq!(err, "the named parameter :advice has no value");
    }

    #[test]
    fn a_mapping_with_an_unused_key_names_the_key() {
        let params = named(&[("a", Value::Null), ("z", Value::Null), ("y", Value::Null)]);
        let err = plan(&[Some(":a")], &params).unwrap_err();
        assert_eq!(err, "the parameter \"y\" is not a placeholder in the query");
    }

    #[test]
    fn a_mapping_key_carrying_the_prefix_is_unused() {
        let err = plan(&[], &named(&[(":a", Value::Null)])).unwrap_err();
        assert_eq!(
            err,
            "the parameter \":a\" is not a placeholder in the query"
        );
    }

    #[test]
    fn a_mapping_for_a_bare_placeholder_is_refused() {
        let err = plan(&[None], &named(&[])).unwrap_err();
        assert_eq!(
            err,
            "the query uses the positional parameter ?; pass a sequence, not a mapping"
        );
    }

    #[test]
    fn a_mapping_for_a_numbered_placeholder_is_refused() {
        let err = plan(&[Some("?2")], &named(&[])).unwrap_err();
        assert_eq!(
            err,
            "the query uses the positional parameter ?2; pass a sequence, not a mapping"
        );
    }
}
