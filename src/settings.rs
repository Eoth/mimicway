//! Settings read from environment variables.
//!
//! A setting is computed from a lookup, a function from a variable's name to its value: the server passes `env`, the
//! process environment, and tests pass `vars`, their own values. Tests never change the process environment: every
//! test thread shares it, so a value one test sets is seen by the tests running beside it. Changing it is `unsafe`
//! since edition 2024, and the crate forbids `unsafe` code in its tests as in the binary, so the compiler refuses a
//! test that tries.
use std::str::FromStr;

/// The lookup the server runs with: the process environment. A value that is not valid UTF-8 reads as unset.
pub fn env(name: &str) -> Option<String> {
    std::env::var(name).ok()
}

/// A number read through `lookup`, or `default` when the variable is unset or is not a number.
pub fn number<T: FromStr>(lookup: impl Fn(&str) -> Option<String>, name: &str, default: T) -> T {
    lookup(name)
        .and_then(|value| value.parse().ok())
        .unwrap_or(default)
}

/// A lookup that knows only `pairs`, for tests.
#[cfg(test)]
pub fn vars(pairs: &[(&str, &str)]) -> impl Fn(&str) -> Option<String> + use<> {
    let values: std::collections::HashMap<String, String> = pairs
        .iter()
        .map(|(name, value)| (name.to_string(), value.to_string()))
        .collect();
    move |name: &str| values.get(name).cloned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_number_falls_back_to_its_default_when_unset_or_not_a_number() {
        assert_eq!(number(vars(&[]), "LIMIT", 7usize), 7);
        assert_eq!(number(vars(&[("LIMIT", "12")]), "LIMIT", 7usize), 12);
        assert_eq!(number(vars(&[("LIMIT", "twelve")]), "LIMIT", 7usize), 7);
        assert_eq!(number(vars(&[("LIMIT", "-1")]), "LIMIT", 7usize), 7);
    }

    #[test]
    fn a_test_lookup_knows_only_its_own_variables() {
        let lookup = vars(&[("PATH", "/only/here")]);
        assert_eq!(lookup("PATH").as_deref(), Some("/only/here"));
        assert_eq!(lookup("HOME"), None);
    }
}
