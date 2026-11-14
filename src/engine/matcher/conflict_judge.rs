// The judge of conflict detection: the production matcher itself. Two generated rules are put in order, every
// request of a finite universe goes through `first_match`, and the truth is what it answers: the second rule is hidden
// when it can answer some request on its own but never once the first one stands before it. `find_rule_conflicts`
// must report exactly those pairs:
// - never a pair where the second rule still answers some request behind the first (a false alarm: the order already
//   settles it); a request of the universe that proves it is a real request;
// - every hidden pair whose conditions have no regular expression other than a plain text (the decision procedure is
//   complete there; with a real regular expression it may stay silent, never wrong).
//
// The universe holds, for each field the two rules read, the absent value and every value that can tell two rules
// apart: all texts of length 0 to 2 over `a`, `b` and a separator `#` that no condition names, and a few longer ones
// that join two constants; every path, with each constant in each segment a pattern captures. Fields the rules do
// not read keep one value: a condition only reads its own source.
//
// What it does not see: fields read from the body (JSON pointer, XPath, form field, raw body), header names in other
// cases than `h`/`H`, encoded paths, and a hidden rule only covered by the union of several earlier rules (detection
// works pair by pair).
use super::*;
use crate::models::*;
use proptest::prelude::*;

const VALUES: &[&str] = &[
    "", "a", "b", "#", "aa", "ab", "a#", "ba", "bb", "b#", "#a", "#b", "##", "ab#", "#ab", "a#b",
    "aba", "#a#", "b#a",
];
// Every constant a condition may compare a path parameter with stands in each segment a pattern can capture.
const PATHS: &[&str] = &[
    "", "/x", "/y", "/a", "/b", "/ab", "/x/y", "/x/a", "/x/b", "/x/ab", "/a/y", "/b/y", "/ab/y",
    "/x/y/z",
];
const METHODS: &[&str] = &["GET", "POST"];

fn arb_operator() -> impl Strategy<Value = Operator> {
    let text = prop_oneof![Just(""), Just("a"), Just("b"), Just("ab")].prop_map(String::from);
    prop_oneof![
        Just(Operator::Exists),
        text.clone().prop_map(Operator::Eq),
        text.prop_map(Operator::Contains),
        prop_oneof![
            Just("a"),
            Just("ab"),
            Just("^a"),
            Just("b$"),
            Just("^(a|b)$"),
            Just("[")
        ]
        .prop_map(|p| Operator::Regex(p.into())),
    ]
}

/// The two fields a pair of rules reads, out of a query parameter, a header and a path parameter: with a third one the
/// universe, a product over fields, takes minutes.
fn arb_sources() -> impl Strategy<Value = [ConditionSource; 3]> {
    let q = ConditionSource::QueryParam("q".into());
    let h = |name: &str| ConditionSource::Header(name.into());
    let id = ConditionSource::PathParam("id".into());
    prop_oneof![
        Just([q.clone(), h("h"), h("H")]),
        Just([q.clone(), id.clone(), q.clone()]),
        Just([h("H"), id.clone(), h("h")]),
    ]
}

fn arb_condition(sources: [ConditionSource; 3]) -> impl Strategy<Value = Condition> {
    (prop::sample::select(sources.to_vec()), arb_operator())
        .prop_map(|(source, operator)| Condition { source, operator })
}

fn arb_pair() -> impl Strategy<Value = (Rule, Rule)> {
    arb_sources().prop_flat_map(|sources| {
        (
            arb_rule("first", sources.clone()),
            arb_rule("second", sources),
        )
    })
}

fn arb_rule(name: &'static str, sources: [ConditionSource; 3]) -> impl Strategy<Value = Rule> {
    let sub_path = prop_oneof![
        Just(None),
        prop_oneof![
            Just("/x"),
            Just("/y"),
            Just("/{id}"),
            Just("/:id"),
            Just("/x/{id}"),
            Just("/{id}/y"),
            Just("/x/*"),
            Just("/*"),
            Just("/{id}/*"),
            Just("/x/y"),
        ]
        .prop_map(|p| Some(p.to_string())),
    ];
    (
        prop_oneof![Just("GET"), Just("POST"), Just("get")],
        sub_path,
        prop::collection::vec(arb_condition(sources.clone()), 0..=2),
        prop::collection::vec(arb_condition(sources), 0..=2),
    )
        .prop_map(move |(method, sub_path, all_of, any_of)| Rule {
            name: name.into(),
            method: method.into(),
            sub_path,
            action: RuleAction::Mock,
            script: None,
            response_mode: None,
            conditions: ConditionGroup { all_of, any_of },
            response: MockResponse {
                status: 200,
                headers: vec![],
                body: vec![],
                chaos: None,
                events: None,
            },
        })
}

fn reads(rules: &[&Rule], wanted: fn(&ConditionSource) -> bool) -> bool {
    rules.iter().any(|r| {
        r.conditions
            .all_of
            .iter()
            .chain(&r.conditions.any_of)
            .any(|c| wanted(&c.source))
    })
}

/// Every request of the universe, for the fields `rules` read.
fn universe(rules: &[&Rule]) -> Vec<RequestData> {
    let field_values = |read: bool| -> Vec<Option<&str>> {
        if read {
            std::iter::once(None)
                .chain(VALUES.iter().copied().map(Some))
                .collect()
        } else {
            vec![None]
        }
    };
    let queries = field_values(reads(rules, |s| {
        matches!(s, ConditionSource::QueryParam(_))
    }));
    let headers = field_values(reads(rules, |s| matches!(s, ConditionSource::Header(_))));
    let listen = field_values(reads(rules, |s| matches!(s, ConditionSource::PathParam(_))));
    let mut out = Vec::new();
    for method in METHODS {
        for path in PATHS {
            for l in &listen {
                for q in &queries {
                    for h in &headers {
                        let map = |key: &str, v: &Option<&str>| -> HashMap<String, String> {
                            v.iter().map(|v| (key.to_string(), v.to_string())).collect()
                        };
                        out.push(RequestData {
                            query_params: map("q", q),
                            headers: map("h", h),
                            body: Vec::new(),
                            content_type: None,
                            path_params: map("id", l),
                            method: (*method).into(),
                            remaining_path: (*path).into(),
                        });
                    }
                }
            }
        }
    }
    out
}

/// The truth, from `first_match` alone: whether `second` answers some request by itself, and whether it answers one
/// behind `first`.
fn truth(first: &Rule, second: &Rule) -> (bool, bool) {
    truth_over(first, second, &universe(&[first, second]))
}

fn truth_over(first: &Rule, second: &Rule, requests: &[RequestData]) -> (bool, bool) {
    let alone = std::slice::from_ref(second);
    let both = [first.clone(), second.clone()];
    let answers_alone = requests
        .iter()
        .any(|r| MatchEngine::first_match(alone, r).is_some());
    let answers_behind = requests.iter().any(|r| {
        MatchEngine::first_match(&both, r).is_some_and(|(rule, _)| rule.name == second.name)
    });
    (answers_alone, answers_behind)
}

fn has_real_regex(rule: &Rule) -> bool {
    rule.conditions
        .all_of
        .iter()
        .chain(&rule.conditions.any_of)
        .any(|c| matches!(&c.operator, Operator::Regex(p) if regex::escape(p) != *p))
}

/// What detection reports for `first` then `second`, the draft being one or the other.
fn reported(first: &Rule, second: &Rule, draft_is_first: bool) -> Option<ConflictWinner> {
    let (draft, other, position) = if draft_is_first {
        (first, second, 0)
    } else {
        (second, first, 1)
    };
    let draft = RuleConflictDraft {
        method: &draft.method,
        sub_path: &draft.sub_path,
        conditions: &draft.conditions,
    };
    let other = OtherRuleConflictInput {
        name: &other.name,
        method: &other.method,
        sub_path: &other.sub_path,
        conditions: &other.conditions,
    };
    let conflicts = MatchEngine::find_rule_conflicts(&draft, &[other], position);
    conflicts.first().map(|c| c.winner)
}

proptest! {
    #![proptest_config(ProptestConfig { cases: 256, ..ProptestConfig::default() })]

    #[test]
    fn conflict_reported_exactly_when_first_match_never_reaches_the_rule(
        (first, second) in arb_pair(),
        draft_is_first in any::<bool>(),
    ) {
        let (answers_alone, answers_behind) = truth(&first, &second);
        let hidden = answers_alone && !answers_behind;
        let winner = reported(&first, &second, draft_is_first);
        let expected_winner = if draft_is_first { ConflictWinner::Draft } else { ConflictWinner::Other };
        // A rule that answers nothing even alone (two conditions no value meets) never triggers anyway: reporting it
        // or not is no false alarm.
        let false_alarm = winner.is_some() && answers_behind;
        prop_assert!(!false_alarm, "reported, but the second rule still answers a request:\n{first:#?}\n{second:#?}");
        let wrong_winner = winner.is_some_and(|w| w != expected_winner);
        prop_assert!(!wrong_winner, "the first rule always wins, got {winner:?}");
        let decidable = !has_real_regex(&first) && !has_real_regex(&second);
        let missed = hidden && decidable && winner.is_none();
        prop_assert!(!missed, "hidden, but not reported:\n{first:#?}\n{second:#?}");
    }
}

fn rule(
    name: &str,
    sub_path: Option<&str>,
    all_of: Vec<Condition>,
    any_of: Vec<Condition>,
) -> Rule {
    Rule {
        name: name.into(),
        method: "GET".into(),
        sub_path: sub_path.map(String::from),
        action: RuleAction::Mock,
        script: None,
        response_mode: None,
        conditions: ConditionGroup { all_of, any_of },
        response: MockResponse {
            status: 200,
            headers: vec![],
            body: vec![],
            chaos: None,
            events: None,
        },
    }
}

/// The verdict of the proptest above on one pair, for either rule as the draft.
fn judge(first: &Rule, second: &Rule, (answers_alone, answers_behind): (bool, bool)) {
    let decidable = !has_real_regex(first) && !has_real_regex(second);
    for draft_is_first in [true, false] {
        let winner = reported(first, second, draft_is_first);
        let expected = if draft_is_first {
            ConflictWinner::Draft
        } else {
            ConflictWinner::Other
        };
        assert!(
            !(winner.is_some() && answers_behind),
            "reported, but the second rule still answers a request:\n{first:#?}\n{second:#?}"
        );
        assert!(
            winner.is_none_or(|w| w == expected),
            "the first rule always wins, got {winner:?}"
        );
        assert!(
            !(answers_alone && !answers_behind && decidable && winner.is_none()),
            "hidden, but not reported:\n{first:#?}\n{second:#?}"
        );
    }
}

// Every pair of rules with at most one condition on the same query parameter, in `all_of` or in `any_of`, out of every
// operator the proptest draws, plain-text and real regular expressions side by side, then every rule with two `all_of`
// conditions against each of those: the drawn pairs leave out most of them (a real expression read as a plain text,
// `^(a|b)$` hidden behind `Contains("a")`, and `Eq("a")` twice read as two different values went unseen).
#[test]
fn every_pair_of_single_condition_rules_is_judged() {
    let mut operators = vec![Operator::Exists];
    for text in ["", "a", "b", "ab"] {
        operators.push(Operator::Eq(text.into()));
        operators.push(Operator::Contains(text.into()));
    }
    for pattern in ["", "a", "ab", "^a", "b$", "^(a|b)$", "a|b", "["] {
        operators.push(Operator::Regex(pattern.into()));
    }
    let q = |operator: &Operator| Condition {
        source: ConditionSource::QueryParam("q".into()),
        operator: operator.clone(),
    };
    let mut groups = vec![(vec![], vec![])];
    for operator in &operators {
        groups.push((vec![q(operator)], vec![]));
        groups.push((vec![], vec![q(operator)]));
    }
    let requests: Vec<RequestData> = std::iter::once(None)
        .chain(VALUES.iter().map(Some))
        .map(|value| RequestData {
            query_params: value
                .iter()
                .map(|v| ("q".to_string(), v.to_string()))
                .collect(),
            headers: HashMap::new(),
            body: Vec::new(),
            content_type: None,
            path_params: HashMap::new(),
            method: "GET".into(),
            remaining_path: "/x".into(),
        })
        .collect();
    for (first_all, first_any) in &groups {
        let first = rule("first", None, first_all.clone(), first_any.clone());
        for (second_all, second_any) in &groups {
            let second = rule("second", None, second_all.clone(), second_any.clone());
            judge(&first, &second, truth_over(&first, &second, &requests));
        }
    }
    // Then two conditions in the `all_of` of one rule, the same twice included, against one condition in the other.
    for a in &operators {
        for b in &operators {
            let two = rule("two", None, vec![q(a), q(b)], vec![]);
            for (all_of, any_of) in &groups {
                let one = rule("one", None, all_of.clone(), any_of.clone());
                judge(&one, &two, truth_over(&one, &two, &requests));
                judge(&two, &one, truth_over(&two, &one, &requests));
            }
        }
    }
}

// Every pair of sub-paths out of patterns `match_path` reads, odd ones included: a brace on one side of a segment
// only (`{x`, `x}`, `{`) is a literal segment there, `{}` captures under an empty name, `%7Bid%7D` is decoded first.
#[test]
fn every_pair_of_sub_paths_is_judged() {
    let patterns = [
        None,
        Some("/x"),
        Some("/y"),
        Some("/{id}"),
        Some("/:id"),
        Some("/{}"),
        Some("/{x"),
        Some("/x}"),
        Some("/{"),
        Some("/}"),
        Some("/%7Bid%7D"),
        Some("/x/{id}"),
        Some("/x/y"),
        Some("/x/*"),
        Some("/*"),
        Some("/{id}/*"),
        Some("/"),
    ];
    // The paths of the universe, and each odd pattern as a path of its own, so that a literal one answers some request.
    let odd = ["/{x", "/x}", "/{", "/}", "/{}", "/{id}", "/x/{id}"];
    let requests: Vec<RequestData> = PATHS
        .iter()
        .chain(&odd)
        .map(|path| RequestData {
            query_params: HashMap::new(),
            headers: HashMap::new(),
            body: Vec::new(),
            content_type: None,
            path_params: HashMap::new(),
            method: "GET".into(),
            remaining_path: (*path).into(),
        })
        .collect();
    for first_path in patterns {
        let first = rule("first", first_path, vec![], vec![]);
        for second_path in patterns {
            let second = rule("second", second_path, vec![], vec![]);
            judge(&first, &second, truth_over(&first, &second, &requests));
        }
    }
}
