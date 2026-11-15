//! Patterns written by users (rule conditions, raw TCP matchers), compiled once and reused. Compiling a regex costs
//! far more than running it, and the same few patterns are evaluated on every request: they used to be compiled
//! again for each one. The cache is bounded (emptied when full; a configuration holds few patterns), and so is the
//! compiled size of each pattern, so that a pattern cannot make the server allocate without limit. Saving a
//! configuration checks its patterns with the same limit, so a pattern accepted then always compiles here.
use std::collections::HashMap;
use std::sync::{Arc, LazyLock, RwLock};

const MAX_CACHED: usize = 1024;
const MAX_COMPILED_SIZE: usize = 1 << 20;

type Cache<R> = LazyLock<RwLock<HashMap<String, Option<Arc<R>>>>>;

static TEXT: Cache<regex::Regex> = LazyLock::new(Default::default);
static BYTES: Cache<regex::bytes::Regex> = LazyLock::new(Default::default);

fn cached<R>(
    cache: &Cache<R>,
    pattern: &str,
    compile: impl FnOnce(&str) -> Option<R>,
) -> Option<Arc<R>> {
    if let Some(found) = cache.read().unwrap_or_else(|e| e.into_inner()).get(pattern) {
        return found.clone();
    }
    let compiled = compile(pattern).map(Arc::new);
    let mut cache = cache.write().unwrap_or_else(|e| e.into_inner());
    if cache.len() >= MAX_CACHED {
        cache.clear();
    }
    cache.insert(pattern.to_string(), compiled.clone());
    compiled
}

pub fn check_text(pattern: &str) -> Result<(), String> {
    regex::RegexBuilder::new(pattern)
        .size_limit(MAX_COMPILED_SIZE)
        .build()
        .map(|_| ())
        .map_err(|e| e.to_string())
}

pub fn check_bytes(pattern: &str) -> Result<(), String> {
    regex::bytes::RegexBuilder::new(pattern)
        .size_limit(MAX_COMPILED_SIZE)
        .build()
        .map(|_| ())
        .map_err(|e| e.to_string())
}

/// The compiled pattern, `None` when it does not compile within the limits.
pub fn text(pattern: &str) -> Option<Arc<regex::Regex>> {
    cached(&TEXT, pattern, |p| {
        regex::RegexBuilder::new(p)
            .size_limit(MAX_COMPILED_SIZE)
            .build()
            .ok()
    })
}

/// Same as `text`, for patterns matched against raw bytes.
pub fn bytes(pattern: &str) -> Option<Arc<regex::bytes::Regex>> {
    cached(&BYTES, pattern, |p| {
        regex::bytes::RegexBuilder::new(p)
            .size_limit(MAX_COMPILED_SIZE)
            .build()
            .ok()
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compiles_a_pattern_once() {
        let first = text("^order-[0-9]+$").unwrap();
        let second = text("^order-[0-9]+$").unwrap();
        assert!(Arc::ptr_eq(&first, &second));
        assert!(first.is_match("order-42"));
    }

    #[test]
    fn invalid_or_oversized_patterns_never_compile() {
        assert!(text("(").is_none());
        assert!(check_text("(").is_err());
        // Fine to write, megabytes once compiled.
        let huge = "[a-z]{1000}{1000}";
        assert!(check_text(huge).is_err());
        assert!(text(huge).is_none());
        assert!(bytes(huge).is_none());
        assert!(check_bytes("^\\x02").is_ok());
    }

    /// Every pattern of up to three characters among a few that make or break a regex, against the regex crate
    /// without limits: small patterns are far from the size limit, so the check accepts what compiles, the cache
    /// gives a compiled pattern exactly then, and it matches what the regex crate's own one matches.
    #[test]
    fn short_patterns_compile_and_match_as_the_regex_crate_says() {
        let alphabet = ['a', '(', ')', '[', ']', '*', '\\', 'x', 'F', '^'];
        let haystacks = ["", "a", "aa", "x", "(a)", "*", "F", "\\xF"];
        let mut patterns = vec![String::new()];
        let mut last = vec![String::new()];
        for _ in 0..3 {
            last = last
                .iter()
                .flat_map(|prefix| alphabet.iter().map(move |c| format!("{prefix}{c}")))
                .collect();
            patterns.extend(last.iter().cloned());
        }
        let (mut texts, mut bytes_patterns) = (0, 0);
        for pattern in &patterns {
            let oracle = regex::Regex::new(pattern).ok();
            assert_eq!(check_text(pattern).is_ok(), oracle.is_some(), "{pattern:?}");
            let compiled = text(pattern);
            assert_eq!(compiled.is_some(), oracle.is_some(), "{pattern:?}");
            if let (Some(compiled), Some(oracle)) = (compiled, oracle) {
                texts += 1;
                for haystack in haystacks {
                    assert_eq!(
                        compiled.is_match(haystack),
                        oracle.is_match(haystack),
                        "{pattern:?} on {haystack:?}"
                    );
                }
            }
            let oracle = regex::bytes::Regex::new(pattern).ok();
            assert_eq!(
                check_bytes(pattern).is_ok(),
                oracle.is_some(),
                "{pattern:?}"
            );
            let compiled = bytes(pattern);
            assert_eq!(compiled.is_some(), oracle.is_some(), "{pattern:?}");
            if let (Some(compiled), Some(oracle)) = (compiled, oracle) {
                bytes_patterns += 1;
                for haystack in haystacks {
                    let haystack = haystack.as_bytes();
                    assert_eq!(
                        compiled.is_match(haystack),
                        oracle.is_match(haystack),
                        "{pattern:?} on {haystack:?}"
                    );
                }
            }
        }
        // Both answers occur: neither side of any comparison above is always the same.
        assert!(
            texts > 0 && texts < patterns.len(),
            "{texts} of {}",
            patterns.len()
        );
        assert!(bytes_patterns > 0 && bytes_patterns < patterns.len());
    }

    #[test]
    fn a_full_cache_is_emptied_before_the_next_pattern() {
        let cache: Cache<usize> = LazyLock::new(Default::default);
        for i in 0..MAX_CACHED {
            assert_eq!(
                cached(&cache, &i.to_string(), |p| p.parse().ok()).as_deref(),
                Some(&i)
            );
        }
        assert_eq!(cache.read().unwrap().len(), MAX_CACHED);
        // Already there: found, nothing compiled and nothing emptied.
        assert_eq!(cached(&cache, "0", |_| unreachable!()).as_deref(), Some(&0));
        assert_eq!(cache.read().unwrap().len(), MAX_CACHED);
        let next = MAX_CACHED.to_string();
        assert_eq!(
            cached(&cache, &next, |p| p.parse().ok()).as_deref(),
            Some(&MAX_CACHED)
        );
        let cache = cache.read().unwrap();
        assert_eq!(cache.len(), 1);
        assert!(cache.contains_key(&next));
    }
}
