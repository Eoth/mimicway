use super::*;
use crate::models::{Condition, ConditionGroup, ConditionSource, Operator};

fn query(key: &str, op: Operator) -> Condition {
    Condition {
        source: ConditionSource::QueryParam(key.into()),
        operator: op,
    }
}

fn eq(text: &str) -> Operator {
    Operator::Eq(text.into())
}

fn shape<'a>(
    method: &'a str,
    sub_path: &'a Option<String>,
    conditions: &'a ConditionGroup,
) -> RuleShape<'a> {
    RuleShape {
        method,
        sub_path,
        conditions,
    }
}

fn group(all_of: Vec<Condition>, any_of: Vec<Condition>) -> ConditionGroup {
    ConditionGroup { all_of, any_of }
}

#[test]
fn a_specific_rule_before_a_general_one_hides_nothing() {
    let specific = group(vec![query("env", eq("prod"))], vec![]);
    let general = group(vec![], vec![]);
    assert!(!hides(
        &shape("GET", &None, &specific),
        &shape("GET", &None, &general)
    ));
    assert!(hides(
        &shape("GET", &None, &general),
        &shape("GET", &None, &specific)
    ));
}

#[test]
fn a_rule_with_more_conditions_after_one_with_fewer_is_hidden() {
    let fewer = group(vec![query("env", eq("prod"))], vec![]);
    let more = group(
        vec![query("env", eq("prod")), query("id", Operator::Exists)],
        vec![],
    );
    assert!(hides(
        &shape("GET", &None, &fewer),
        &shape("get", &None, &more)
    ));
    assert!(!hides(
        &shape("GET", &None, &more),
        &shape("GET", &None, &fewer)
    ));
    assert!(!hides(
        &shape("POST", &None, &fewer),
        &shape("GET", &None, &more)
    ));
}

#[test]
fn an_equal_value_decides_the_other_operators_of_its_field() {
    let starts = group(vec![query("id", Operator::Regex("^ab".into()))], vec![]);
    let exact = group(vec![query("id", eq("abc"))], vec![]);
    let other = group(vec![query("id", eq("xbc"))], vec![]);
    assert!(hides(
        &shape("GET", &None, &starts),
        &shape("GET", &None, &exact)
    ));
    assert!(!hides(
        &shape("GET", &None, &starts),
        &shape("GET", &None, &other)
    ));
}

#[test]
fn any_of_is_covered_one_alternative_at_a_time() {
    let either = group(
        vec![],
        vec![query("a", Operator::Exists), query("b", Operator::Exists)],
    );
    let only_a = group(vec![query("a", eq("1"))], vec![]);
    let a_or_c = group(
        vec![],
        vec![query("a", eq("1")), query("c", Operator::Exists)],
    );
    assert!(hides(
        &shape("GET", &None, &either),
        &shape("GET", &None, &only_a)
    ));
    assert!(!hides(
        &shape("GET", &None, &either),
        &shape("GET", &None, &a_or_c)
    ));
}

#[test]
fn contains_is_hidden_unless_the_excluded_text_can_be_avoided() {
    let has_ab = group(vec![query("q", Operator::Contains("ab".into()))], vec![]);
    let has_a = group(vec![query("q", Operator::Contains("a".into()))], vec![]);
    let has_b = group(vec![query("q", Operator::Contains("b".into()))], vec![]);
    assert!(hides(
        &shape("GET", &None, &has_a),
        &shape("GET", &None, &has_ab)
    ));
    assert!(!hides(
        &shape("GET", &None, &has_ab),
        &shape("GET", &None, &has_a)
    ));
    assert!(!hides(
        &shape("GET", &None, &has_b),
        &shape("GET", &None, &has_a)
    ));
}

#[test]
fn a_rule_that_matches_nothing_is_not_hidden_by_another() {
    let any = group(vec![], vec![]);
    let impossible = group(vec![query("q", eq("a")), query("q", eq("b"))], vec![]);
    assert!(!hides(
        &shape("GET", &None, &any),
        &shape("GET", &None, &impossible)
    ));
    let empty_pattern = Some("/".to_string());
    assert!(!hides(
        &shape("GET", &None, &any),
        &shape("GET", &empty_pattern, &any)
    ));
}

#[test]
fn sub_paths_are_compared_as_sets_of_paths() {
    let any = group(vec![], vec![]);
    let p = |s: &str| Some(s.to_string());
    let (star, orders, order, order_lines, other) = (
        p("/orders/*"),
        p("/orders"),
        p("/orders/{id}"),
        p("/orders/:id/lines"),
        p("/users/{id}"),
    );
    assert!(hides(
        &shape("GET", &star, &any),
        &shape("GET", &order_lines, &any)
    ));
    assert!(hides(
        &shape("GET", &star, &any),
        &shape("GET", &orders, &any)
    ));
    assert!(hides(
        &shape("GET", &None, &any),
        &shape("GET", &order, &any)
    ));
    assert!(!hides(
        &shape("GET", &order, &any),
        &shape("GET", &star, &any)
    ));
    assert!(!hides(
        &shape("GET", &order, &any),
        &shape("GET", &other, &any)
    ));
    assert!(!hides(
        &shape("GET", &order, &any),
        &shape("GET", &None, &any)
    ));
}

#[test]
fn a_path_parameter_condition_reads_the_segment_its_rule_captures() {
    let p = |s: &str| Some(s.to_string());
    let param_is = |v: &str| {
        group(
            vec![Condition {
                source: ConditionSource::PathParam("id".into()),
                operator: eq(v),
            }],
            vec![],
        )
    };
    let (by_param, by_literal, elsewhere) = (p("/{id}/lines"), p("/42/lines"), p("/42/{id}"));
    let is_42 = param_is("42");
    let any = group(vec![], vec![]);
    assert!(hides(
        &shape("GET", &by_param, &is_42),
        &shape("GET", &by_literal, &any)
    ));
    // Same name, another segment: `id` of the later rule is the second segment, not the first.
    assert!(!hides(
        &shape("GET", &by_param, &is_42),
        &shape("GET", &elsewhere, &is_42)
    ));
    // No sub-path: the condition reads the listen path parameter, which the later rule does not fix.
    assert!(!hides(
        &shape("GET", &None, &is_42),
        &shape("GET", &by_literal, &any)
    ));
}

#[test]
fn a_real_regular_expression_is_only_proven_against_itself() {
    let digits = group(vec![query("q", Operator::Regex("^[0-9]+$".into()))], vec![]);
    let digit = group(vec![query("q", Operator::Regex("^[0-9]$".into()))], vec![]);
    assert!(hides(
        &shape("GET", &None, &digits),
        &shape("GET", &None, &digits)
    ));
    // True in fact, not proven: silent rather than wrong.
    assert!(!hides(
        &shape("GET", &None, &digits),
        &shape("GET", &None, &digit)
    ));
}
