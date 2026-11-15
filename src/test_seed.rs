// The seeds of the tests that draw their cases: a property test (proptest), or a test that draws through the
// server's own random generator (fake data, chaos mode), takes its seed from the one written beside it, so that the
// CI, the mutation gate and a run by hand judge the same cases, and a test turns red on the commit that breaks it and
// stays red when run again.
//
// A seed drawn on every run would keep finding new cases, but it turns the CI red on whatever commit runs next (a
// dependency update did, with the conflict judge), the failing case is lost with the runner, and a mutant can survive
// one week and be caught the next. New cases are explored by hand instead, when a judge or what it judges changes:
// `PROPTEST_RNG_SEED=<n>`, a decimal number, replaces every written seed, so that
// `for n in $(seq 1 200); do PROPTEST_RNG_SEED=$n cargo test --lib -q <test> || echo "seed $n"; done`
// prints the seeds that fail; each one replays the same way, and its case becomes a test of its own. Some judges
// also count their cases on each side of a limit; those counts hold for the written seed, and another seed can leave
// one short without any defect (the message says which).
//
// Other variables read by proptest change the cases too (`PROPTEST_CASES` inside a `proptest!` block,
// `PROPTEST_MAX_DEFAULT_SIZE_RANGE`, `PROPTEST_RNG_ALGORITHM`); the CI sets none of them.
//
// scripts/proptest-seeds.test.mjs refuses a property test that does not take its configuration from here; it does
// not see a test that draws through the server's random generator without `draw_from`.
use proptest::test_runner::{Config, RngSeed};

/// `cases` cases drawn from `seed`, or from `PROPTEST_RNG_SEED`; a failure writes no file.
pub(crate) fn config(cases: u32, seed: u64) -> Config {
    Config {
        cases,
        rng_seed: RngSeed::Fixed(chosen(explored().as_deref(), seed)),
        failure_persistence: None,
        ..Config::default()
    }
}

/// Seeds the server's random generator (fastrand, one per thread, so one per test) with `seed`, or with
/// `PROPTEST_RNG_SEED`.
pub(crate) fn draw_from(seed: u64) {
    fastrand::seed(chosen(explored().as_deref(), seed));
}

fn explored() -> Option<String> {
    match std::env::var("PROPTEST_RNG_SEED") {
        Ok(text) => Some(text),
        Err(std::env::VarError::NotPresent) => None,
        Err(std::env::VarError::NotUnicode(text)) => Some(text.to_string_lossy().into_owned()),
    }
}

/// A seed set but unreadable stops the test: falling back on the written one would explore nothing, green.
fn chosen(explored: Option<&str>, written: u64) -> u64 {
    match explored {
        None => written,
        Some(text) => text.parse().unwrap_or_else(|_| {
            panic!(
                "PROPTEST_RNG_SEED={text:?} is not a decimal number from 0 to {}",
                u64::MAX
            )
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_written_seed_is_drawn_from_unless_another_one_is_explored() {
        assert_eq!(chosen(None, 0x5eed), 0x5eed);
        assert_eq!(chosen(Some("33"), 0x5eed), 33);
        assert_eq!(chosen(Some("0"), 0x5eed), 0);
        assert_eq!(chosen(Some("18446744073709551615"), 0x5eed), u64::MAX);
        for unreadable in ["", "0x10", "-1", " 33", "18446744073709551616", "seed"] {
            let refused = std::panic::catch_unwind(|| chosen(Some(unreadable), 0x5eed));
            assert!(refused.is_err(), "{unreadable:?}");
        }
        let config = config(7, 0x5eed);
        assert_eq!(config.cases, 7);
        assert_eq!(
            config.rng_seed,
            RngSeed::Fixed(chosen(explored().as_deref(), 0x5eed))
        );
        assert!(config.failure_persistence.is_none());
    }

    #[test]
    fn the_server_draws_from_the_chosen_seed() {
        for seed in [1, 2, 0x5eed] {
            draw_from(seed);
            let drawn: Vec<u64> = (0..8).map(|_| fastrand::u64(..)).collect();
            let mut expected = fastrand::Rng::with_seed(chosen(explored().as_deref(), seed));
            assert_eq!(drawn, (0..8).map(|_| expected.u64(..)).collect::<Vec<_>>());
        }
    }
}
