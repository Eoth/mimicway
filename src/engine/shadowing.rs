//! Whether an earlier rule leaves a later rule of the same service nothing to answer (`hides`), for the conflict
//! warning of the rule form (`MatchEngine::find_rule_conflicts`). Rules are tried in order and the first match wins
//! (`MatchEngine::first_match`), so two rules that share requests are not a mistake by themselves: a specific rule
//! before a general one is the usual way to write a fallback. Only one case is: a later rule whose every request is
//! taken by an earlier one never triggers. `hides(earlier, later)` answers exactly that question, pair by pair.
//!
//! **Model.** A request is seen through the fields the conditions read, each absent or a text: a query parameter, a
//! header (by its lowercase name), a JSON pointer, an XPath, a form field, the raw body, and a path parameter, bound to
//! the segment its rule's sub-path captures it from (`Segment(i)`) or else to the listen path (`Listen(key)`), as
//! `MatchEngine::extract` reads it. The fields are taken as independent of one another. Real requests are a subset of
//! this space (the body fields are tied together in reality), so an inclusion proven here holds for real requests:
//! the warning is never false. The reverse may miss a case (`/a` in a JSON pointer and `<a>` in an XPath of the same
//! body), and the warning then stays silent.
//!
//! **Decision.** A rule is `P ∧ A ∧ (∨Y)`: its sub-path `P`, its `all_of` `A` and its `any_of` `Y` (true when empty).
//! The later rule `L` is hidden by the earlier `E` when its sub-path is within `E`'s (`PathShape::within`) and
//! `L ∧ ¬E` is unsatisfiable, with `¬E = ∨{¬a : a ∈ A_E} ∨ ∧{¬y : y ∈ Y_E}`. Since `L` is a disjunction of conjunctions
//! `D_y = A_L ∧ y` (one per `y ∈ Y_L`, or `A_L` alone), `L ∧ ¬E` is unsatisfiable when every conjunction `D_y ∧ ¬a` and
//! `D_y ∧ ∧¬Y_E` is: `|Y_L| · (|A_E| + 1)` conjunctions of literals at most, never the exponential expansion of `E`'s
//! negation. With independent fields, a conjunction is unsatisfiable exactly when the literals of one field are
//! (`field_unsatisfiable`). `L` must also accept some request: a rule that matches nothing is no other rule's doing.
//!
//! **One field.** Every operator is false on an absent value, so literals with no positive one hold on an absent value.
//! Otherwise the value is present; with a positive `Eq(c)`, the value is `c`, and each literal is decided by
//! `MatchEngine::apply_op` itself, the operator the matcher runs. Without `Eq`, the literals are `Contains(s_i)`,
//! `¬Contains(t_j)` and `¬Eq(c_k)` (a regular expression with no special character is `Contains`), and they hold
//! together exactly when no `t_j` is empty or a part of some `s_i`: the text `s_1 σ s_2 σ … s_n σσ…`, with a character
//! `σ` that no constant holds, repeated past the longest `c_k`, contains each `s_i`, differs from each `c_k`, and any of
//! its parts without `σ` lies within one `s_i`. A real regular expression is only compared with itself (`r ∧ ¬r`) and
//! with an invalid pattern (never matched): past that the procedure answers "not proven", so it may miss a hidden
//! rule, never report one that triggers.
//!
//! Pair by pair: a later rule only covered by the union of several earlier rules is not reported.
//! `matcher/conflict_judge.rs` judges all of this against `first_match` on generated rules.
use crate::engine::matcher::{MatchEngine, normalize_colon_syntax, url_decode};
use crate::models::{Condition, ConditionGroup, ConditionSource, Operator};
use std::borrow::Cow;

/// A rule as conflict detection sees it: what decides whether it matches, nothing of what it answers.
pub(crate) struct RuleShape<'a> {
    pub method: &'a str,
    pub sub_path: &'a Option<String>,
    pub conditions: &'a ConditionGroup,
}

/// True when every request `later` accepts is accepted by `earlier`, and `later` accepts some request: placed after
/// `earlier`, `later` never triggers.
pub(crate) fn hides(earlier: &RuleShape, later: &RuleShape) -> bool {
    if !earlier.method.eq_ignore_ascii_case(later.method) {
        return false;
    }
    let (Some(e_path), Some(l_path)) = (
        PathShape::of(earlier.sub_path),
        PathShape::of(later.sub_path),
    ) else {
        return false;
    };
    if !l_path.within(&e_path) {
        return false;
    }

    let mut l_all: Vec<Literal> = l_path.segment_literals();
    l_all.extend(literals_of(&later.conditions.all_of, &l_path));
    let l_any: Vec<Literal> = literals_of(&later.conditions.any_of, &l_path).collect();
    let e_all: Vec<Literal> = literals_of(&earlier.conditions.all_of, &e_path)
        .map(Literal::negated)
        .collect();
    let e_any: Vec<Literal> = literals_of(&earlier.conditions.any_of, &e_path)
        .map(Literal::negated)
        .collect();

    // The conjunctions of L, one per any_of condition (or all_of alone when any_of is empty).
    let disjuncts: Vec<Vec<&Literal>> = if l_any.is_empty() {
        vec![l_all.iter().collect()]
    } else {
        l_any
            .iter()
            .map(|y| l_all.iter().chain(std::iter::once(y)).collect())
            .collect()
    };
    let accepts_some = disjuncts.iter().any(|d| !unsatisfiable(d));
    accepts_some
        && disjuncts.iter().all(|d| {
            e_all
                .iter()
                .all(|not_a| unsatisfiable_with(d, std::iter::once(not_a)))
                && (e_any.is_empty() || unsatisfiable_with(d, e_any.iter()))
        })
}

/// The literals of `conditions`, their path parameters bound through `path`.
fn literals_of<'a>(
    conditions: &'a [Condition],
    path: &'a PathShape,
) -> impl Iterator<Item = Literal<'a>> {
    conditions.iter().map(move |c| Literal {
        field: path.field_of(&c.source),
        positive: true,
        test: Cow::Borrowed(&c.operator),
    })
}

/// What a condition reads, as `MatchEngine::extract` finds it.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Field<'a> {
    Query(&'a str),
    Header(String),
    Segment(usize),
    Listen(&'a str),
    Json(&'a str),
    XPath(&'a str),
    Form(&'a str),
    Body,
}

#[derive(Debug, Clone)]
struct Literal<'a> {
    field: Field<'a>,
    positive: bool,
    test: Cow<'a, Operator>,
}

impl Literal<'_> {
    fn negated(self) -> Self {
        Literal {
            positive: !self.positive,
            ..self
        }
    }
}

fn unsatisfiable(lits: &[&Literal]) -> bool {
    unsatisfiable_with(lits, std::iter::empty())
}

fn unsatisfiable_with<'b, 'a: 'b>(
    base: &[&'b Literal<'a>],
    extra: impl Iterator<Item = &'b Literal<'a>>,
) -> bool {
    let all: Vec<&Literal> = base.iter().copied().chain(extra).collect();
    all.iter().enumerate().any(|(i, lit)| {
        let first_of_field = all[..i].iter().all(|other| other.field != lit.field);
        if !first_of_field {
            return false;
        }
        let same: Vec<&Literal> = all[i..]
            .iter()
            .copied()
            .filter(|l| l.field == lit.field)
            .collect();
        field_unsatisfiable(&same)
    })
}

/// A text the operator tests the presence of, when it is a plain substring test.
fn substring_of(op: &Operator) -> Option<&str> {
    match op {
        Operator::Contains(s) => Some(s),
        Operator::Regex(p) if regex::escape(p) == *p => Some(p),
        _ => None,
    }
}

/// Exact for `Eq`, `Contains`, `Exists` and plain-text regular expressions; for any other regular expression, true
/// only on a proof (see the module documentation).
fn field_unsatisfiable(lits: &[&Literal]) -> bool {
    let (positives, negatives): (Vec<&Literal>, Vec<&Literal>) =
        lits.iter().partition(|l| l.positive);
    if positives.is_empty() {
        return false;
    }
    if negatives
        .iter()
        .any(|l| matches!(*l.test, Operator::Exists))
    {
        return true;
    }
    let mut equal_to: Option<&str> = None;
    for l in &positives {
        if let Operator::Eq(c) = &*l.test {
            if equal_to.is_some_and(|prev| prev != c) {
                return true;
            }
            equal_to = Some(c);
        }
    }
    if let Some(value) = equal_to {
        return lits
            .iter()
            .any(|l| MatchEngine::apply_op(&l.test, Some(value)) != l.positive);
    }
    let contained: Vec<&str> = positives
        .iter()
        .filter_map(|l| substring_of(&l.test))
        .collect();
    let excluded_part = negatives
        .iter()
        .filter_map(|l| substring_of(&l.test))
        .any(|t| t.is_empty() || contained.iter().any(|s| s.contains(t)));
    if excluded_part {
        return true;
    }
    positives.iter().any(|p| match &*p.test {
        Operator::Regex(pattern) if substring_of(&p.test).is_none() => {
            crate::engine::regex_cache::text(pattern).is_none()
                || negatives
                    .iter()
                    .any(|n| matches!(&*n.test, Operator::Regex(q) if q == pattern))
        }
        _ => false,
    })
}

#[derive(Debug, PartialEq)]
enum Token {
    Literal(String),
    Param(String),
}

/// A sub-path pattern as `match_path` reads it: decoded segments up to the first `*`, which accepts any rest (none
/// included). No sub-path is `open` with no segment.
#[derive(Debug)]
struct PathShape {
    tokens: Vec<Token>,
    open: bool,
}

impl PathShape {
    /// `None` for a pattern with no segment, which `match_path` never matches.
    fn of(sub_path: &Option<String>) -> Option<Self> {
        let Some(pattern) = sub_path else {
            return Some(PathShape {
                tokens: Vec::new(),
                open: true,
            });
        };
        let segments: Vec<String> = normalize_colon_syntax(pattern)
            .split('/')
            .filter(|s| !s.is_empty())
            .map(url_decode)
            .collect();
        if segments.is_empty() {
            return None;
        }
        let mut tokens = Vec::new();
        let mut open = false;
        for seg in segments {
            if seg == "*" {
                open = true;
                break;
            }
            tokens.push(
                if seg.starts_with('{') && seg.ends_with('}') && seg.len() >= 2 {
                    Token::Param(seg[1..seg.len() - 1].to_string())
                } else {
                    Token::Literal(seg)
                },
            );
        }
        Some(PathShape { tokens, open })
    }

    /// Every path `self` accepts is accepted by `outer`.
    fn within(&self, outer: &PathShape) -> bool {
        let long_enough = if outer.open {
            self.tokens.len() >= outer.tokens.len()
        } else {
            !self.open && self.tokens.len() == outer.tokens.len()
        };
        long_enough
            && outer
                .tokens
                .iter()
                .zip(&self.tokens)
                .all(|pair| match pair {
                    (Token::Param(_), _) => true,
                    (Token::Literal(a), Token::Literal(b)) => a == b,
                    (Token::Literal(_), Token::Param(_)) => false,
                })
    }

    /// What the pattern itself says of the segments it accepts: a literal fixes one, a parameter needs it present.
    fn segment_literals(&self) -> Vec<Literal<'static>> {
        self.tokens
            .iter()
            .enumerate()
            .map(|(i, token)| Literal {
                field: Field::Segment(i),
                positive: true,
                test: Cow::Owned(match token {
                    Token::Literal(text) => Operator::Eq(text.clone()),
                    Token::Param(_) => Operator::Exists,
                }),
            })
            .collect()
    }

    /// The field a condition reads; a path parameter comes from the last segment that captures it, as the map built by
    /// `match_path` keeps the last one.
    fn field_of<'a>(&self, source: &'a ConditionSource) -> Field<'a> {
        match source {
            ConditionSource::QueryParam(k) => Field::Query(k),
            ConditionSource::Header(k) => Field::Header(k.to_ascii_lowercase()),
            ConditionSource::JsonPointer(p) => Field::Json(p),
            ConditionSource::XPath(p) => Field::XPath(p),
            ConditionSource::FormField(k) => Field::Form(k),
            ConditionSource::BodyRaw => Field::Body,
            ConditionSource::PathParam(k) => self
                .tokens
                .iter()
                .rposition(|t| matches!(t, Token::Param(name) if name == k))
                .map_or(Field::Listen(k), Field::Segment),
        }
    }
}

#[cfg(test)]
mod tests;
