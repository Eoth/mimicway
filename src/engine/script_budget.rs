// What one run of a rule script may use. Rhai bounds the operations of a run and each value on its own (text length,
// array and map sizes); this module sets those limits and adds what they leave open:
// - a deadline: one operation on a large request body can take tens of milliseconds (parsing, hashing, scanning),
//   so ten thousand of them could keep a worker thread busy for over a minute;
// - a memory budget: values held outside the variables (the arguments of one call, the keys of a map, what `map`
//   gathers, curried function pointers, nested calls) escape Rhai's limits on each value, and reached gigabytes. The
//   global allocator counts what each thread holds (alloc/), and a run is synchronous on its thread, so what the
//   thread holds more than when the run started is what the run holds, however it holds it;
// - a number of variables alive at once;
// - `replace` and the `split` family, checked before they build their result: Rhai's own versions build it whole and
//   check it afterwards, so a single `replace` of a 1 MiB text by a 1 MiB substitute asked for a terabyte at once and
//   aborted the process;
// - error messages that name the limit reached and its value, in the language of the request.
// The native functions of `script.rs` follow the same rule: they stop as soon as their result would break a limit,
// instead of building it and leaving the check to Rhai.
use rhai::{
    Array, Dynamic, EvalAltResult, INT, ImmutableString, ParseError, ParseErrorType, Position,
};
use std::cell::Cell;
use std::time::{Duration, Instant};

pub(crate) const MAX_OPERATIONS: u64 = 10_000;
pub(crate) const MAX_STRING_SIZE: usize = 1_048_576;
pub(crate) const MAX_ARRAY_SIZE: usize = 1_000;
pub(crate) const MAX_MAP_SIZE: usize = 500;
const MAX_CALL_LEVELS: usize = 32;
const MAX_EXPR_DEPTH: usize = 64;
const MAX_FUNCTION_EXPR_DEPTH: usize = 32;
// Every variable alive at once counts: those of the script, loop variables, function parameters and `request`.
// Mock scripts use a handful; the limit is there because each variable may hold a value as large as the limits above.
pub(crate) const MAX_VARIABLES: usize = 64;
// A script that builds a response from a large request runs in a few milliseconds; a hundred times that still leaves
// room for a slow machine, a debug build or a busy one.
pub(crate) const TIME_BUDGET: Duration = Duration::from_millis(500);
// A run holding every value at its limit (a parsed request of 1 MiB of text, 1,000 items and 500 entries, and a
// response built from it) stays under a few MiB; 16 MiB leaves room for that, while the worker threads of a small
// machine running scripts at once stay within the memory a container is usually given.
pub(crate) const MEMORY_BUDGET_MIB: i64 = 16;

// Rhai's names for the values too large (`ErrorDataTooLarge`), which the functions below reuse so that `describe`
// reads both the same way.
pub(crate) const TEXT_TOO_LARGE: &str = "Length of string";
pub(crate) const ARRAY_TOO_LARGE: &str = "Size of array/BLOB";
pub(crate) const MAP_TOO_LARGE: &str = "Size of object map";

// Why on_progress stopped a run: the value it returns becomes the token of Rhai's `ErrorTerminated`.
const OUT_OF_TIME: &str = "time";
const OUT_OF_MEMORY: &str = "memory";

thread_local! {
    // The deadline of the run on this thread, and the bytes the thread held when it started. A run is synchronous: it
    // never leaves its thread before it ends.
    static RUN: Cell<Option<(Instant, i64)>> = const { Cell::new(None) };
}

/// The deadline and the memory budget of one run, set on its thread for as long as the value lives.
pub(crate) struct RunClock;

impl RunClock {
    pub(crate) fn start() -> Self {
        let started = (Instant::now() + TIME_BUDGET, mimicway_alloc::live_bytes());
        RUN.with(|run| run.set(Some(started)));
        RunClock
    }
}

impl Drop for RunClock {
    fn drop(&mut self) {
        RUN.with(|run| run.set(None));
    }
}

pub(crate) fn too_large(kind: &str) -> Box<EvalAltResult> {
    EvalAltResult::ErrorDataTooLarge(kind.to_string(), Position::NONE).into()
}

pub(crate) fn apply(engine: &mut rhai::Engine) {
    engine.set_max_operations(MAX_OPERATIONS);
    engine.set_max_string_size(MAX_STRING_SIZE);
    engine.set_max_array_size(MAX_ARRAY_SIZE);
    engine.set_max_map_size(MAX_MAP_SIZE);
    engine.set_max_call_levels(MAX_CALL_LEVELS);
    engine.set_max_expr_depths(MAX_EXPR_DEPTH, MAX_FUNCTION_EXPR_DEPTH);
    engine.set_max_variables(MAX_VARIABLES);
    engine.on_progress(|operations| {
        let (deadline, held_at_start) = RUN.with(Cell::get)?;
        // The memory at every operation: a thread-local read, and one operation adds at most a value at its limits.
        if mimicway_alloc::live_bytes() - held_at_start > MEMORY_BUDGET_MIB * 1024 * 1024 {
            return Some(OUT_OF_MEMORY.into());
        }
        // The clock every 16 operations: no single operation lasts long (the native functions stop at the limits), and
        // reading it costs as much as a few of them.
        (operations % 16 == 0 && Instant::now() >= deadline).then(|| OUT_OF_TIME.into())
    });
    register_replace(engine);
    register_split(engine);
}

// Functions registered on the engine take precedence over Rhai's standard ones of the same name and parameter types.
fn register_replace(engine: &mut rhai::Engine) {
    engine.register_fn(
        "replace",
        |text: &mut ImmutableString, find: &str, with: &str| replace(text, find, with),
    );
    engine.register_fn(
        "replace",
        |text: &mut ImmutableString, find: &str, with: char| {
            replace(text, find, with.encode_utf8(&mut [0; 4]))
        },
    );
    engine.register_fn(
        "replace",
        |text: &mut ImmutableString, find: char, with: &str| {
            replace(text, find.encode_utf8(&mut [0; 4]), with)
        },
    );
    engine.register_fn(
        "replace",
        |text: &mut ImmutableString, find: char, with: char| {
            replace(
                text,
                find.encode_utf8(&mut [0; 4]),
                with.encode_utf8(&mut [0; 4]),
            )
        },
    );
}

// The length of the result is known before building it: every match (an empty pattern matches between each pair of
// characters, as in `str::replace`) trades the pattern's length for the substitute's.
fn replace(text: &mut ImmutableString, find: &str, with: &str) -> Result<(), Box<EvalAltResult>> {
    if text.is_empty() {
        return Ok(());
    }
    let matches = text.matches(find).count();
    let length =
        (text.len() - matches * find.len()).saturating_add(matches.saturating_mul(with.len()));
    if length > MAX_STRING_SIZE {
        return Err(too_large(TEXT_TOO_LARGE));
    }
    *text = text.replace(find, with).into();
    Ok(())
}

// Same results as Rhai's own `split`, `split_rev` and `to_chars`, but the array stops growing past its limit: a text
// of a million separators no longer becomes a million strings before the size check.
fn register_split(engine: &mut rhai::Engine) {
    engine.register_fn("split", |text: &mut ImmutableString| {
        if text.is_empty() {
            return Ok(vec![text.clone().into()]);
        }
        bounded(text.split_whitespace())
    });
    engine.register_fn("split", |text: &mut ImmutableString, delimiter: &str| {
        if text.is_empty() || (!delimiter.is_empty() && !text.contains(delimiter)) {
            return Ok(vec![text.clone().into()]);
        }
        bounded(text.split(delimiter))
    });
    engine.register_fn(
        "split",
        |text: &mut ImmutableString, delimiter: &str, segments: INT| {
            if segments <= 1
                || text.is_empty()
                || (!delimiter.is_empty() && !text.contains(delimiter))
            {
                return Ok(vec![text.clone().into()]);
            }
            bounded(text.splitn(segments_of(segments), delimiter))
        },
    );
    engine.register_fn("split", |text: &mut ImmutableString, delimiter: char| {
        if text.is_empty() || !text.contains(delimiter) {
            return Ok(vec![text.clone().into()]);
        }
        bounded(text.split(delimiter))
    });
    engine.register_fn(
        "split",
        |text: &mut ImmutableString, delimiter: char, segments: INT| {
            if segments <= 1 || text.is_empty() || !text.contains(delimiter) {
                return Ok(vec![text.clone().into()]);
            }
            bounded(text.splitn(segments_of(segments), delimiter))
        },
    );
    engine.register_fn(
        "split_rev",
        |text: &mut ImmutableString, delimiter: &str| {
            if text.is_empty() || (!delimiter.is_empty() && !text.contains(delimiter)) {
                return Ok(vec![text.clone().into()]);
            }
            bounded(text.rsplit(delimiter))
        },
    );
    engine.register_fn(
        "split_rev",
        |text: &mut ImmutableString, delimiter: &str, segments: INT| {
            if segments <= 1
                || text.is_empty()
                || (!delimiter.is_empty() && !text.contains(delimiter))
            {
                return Ok(vec![text.clone().into()]);
            }
            bounded(text.rsplitn(segments_of(segments), delimiter))
        },
    );
    engine.register_fn(
        "split_rev",
        |text: &mut ImmutableString, delimiter: char| {
            if text.is_empty() || !text.contains(delimiter) {
                return Ok(vec![text.clone().into()]);
            }
            bounded(text.rsplit(delimiter))
        },
    );
    engine.register_fn(
        "split_rev",
        |text: &mut ImmutableString, delimiter: char, segments: INT| {
            if segments <= 1 || text.is_empty() || !text.contains(delimiter) {
                return Ok(vec![text.clone().into()]);
            }
            bounded(text.rsplitn(segments_of(segments), delimiter))
        },
    );
    engine.register_fn(
        "to_chars",
        |text: &str| -> Result<Array, Box<EvalAltResult>> {
            let mut chars = Array::new();
            for c in text.chars() {
                if chars.len() == MAX_ARRAY_SIZE {
                    return Err(too_large(ARRAY_TOO_LARGE));
                }
                chars.push(c.into());
            }
            Ok(chars)
        },
    );
}

/// The text form of a value, as `to_string` gives it, refused past the text limit instead of built whole.
pub(crate) fn display(value: &Dynamic) -> Result<String, Box<EvalAltResult>> {
    use std::fmt::Write;
    let mut out = BoundedText(String::new());
    write!(out, "{value}").map_err(|_| too_large(TEXT_TOO_LARGE))?;
    Ok(out.0)
}

struct BoundedText(String);

impl std::fmt::Write for BoundedText {
    fn write_str(&mut self, text: &str) -> std::fmt::Result {
        if self.0.len() + text.len() > MAX_STRING_SIZE {
            return Err(std::fmt::Error);
        }
        self.0.push_str(text);
        Ok(())
    }
}

fn segments_of(segments: INT) -> usize {
    usize::try_from(segments).unwrap_or(usize::MAX)
}

fn bounded<'a>(parts: impl Iterator<Item = &'a str>) -> Result<Array, Box<EvalAltResult>> {
    let mut array = Array::new();
    for part in parts {
        if array.len() == MAX_ARRAY_SIZE {
            return Err(too_large(ARRAY_TOO_LARGE));
        }
        array.push(part.into());
    }
    Ok(array)
}

/// The message of a failed run. A limit reached is named with its value; any other error keeps Rhai's own text.
pub(crate) fn describe(error: &EvalAltResult) -> String {
    match error {
        EvalAltResult::ErrorTooManyOperations(position) => crate::i18n::tr(
            "Script stopped: it ran more than {0} operations, the limit of a run{1}",
            &[&MAX_OPERATIONS, &at(*position)],
        ),
        EvalAltResult::ErrorTerminated(reason, position) if reason.to_string() == OUT_OF_MEMORY => {
            crate::i18n::tr(
                "Script stopped: it held more than {0} MiB of memory, the memory limit of a run{1}",
                &[&MEMORY_BUDGET_MIB, &at(*position)],
            )
        }
        EvalAltResult::ErrorTerminated(_, position) => crate::i18n::tr(
            "Script stopped: it ran longer than {0} ms, the time limit of a run{1}",
            &[&TIME_BUDGET.as_millis(), &at(*position)],
        ),
        EvalAltResult::ErrorTooManyVariables(position) => crate::i18n::tr(
            "Script stopped: more than {0} variables at once, the limit of a run{1}",
            &[&MAX_VARIABLES, &at(*position)],
        ),
        EvalAltResult::ErrorStackOverflow(position) => crate::i18n::tr(
            "Script stopped: more than {0} nested function calls, the limit of a run{1}",
            &[&MAX_CALL_LEVELS, &at(*position)],
        ),
        EvalAltResult::ErrorDataTooLarge(kind, position) => match kind.as_str() {
            TEXT_TOO_LARGE => crate::i18n::tr(
                "Script stopped: a text longer than {0} bytes, the limit of a value{1}",
                &[&MAX_STRING_SIZE, &at(*position)],
            ),
            ARRAY_TOO_LARGE => crate::i18n::tr(
                "Script stopped: an array of more than {0} items, the limit of a value{1}",
                &[&MAX_ARRAY_SIZE, &at(*position)],
            ),
            MAP_TOO_LARGE => crate::i18n::tr(
                "Script stopped: a map of more than {0} entries, the limit of a value{1}",
                &[&MAX_MAP_SIZE, &at(*position)],
            ),
            _ => error.to_string(),
        },
        EvalAltResult::ErrorParsing(ParseErrorType::ExprTooDeep, position) => too_deep(*position),
        // A limit reached inside a function of the script: the limit is what the user needs to read first.
        EvalAltResult::ErrorInFunctionCall(_, _, inner, _) if is_limit(inner) => describe(inner),
        _ => error.to_string(),
    }
}

/// The message of a script that does not compile.
pub(crate) fn describe_parse(error: &ParseError) -> String {
    match error.err_type() {
        ParseErrorType::ExprTooDeep => too_deep(error.position()),
        _ => error.to_string(),
    }
}

fn too_deep(position: Position) -> String {
    crate::i18n::tr(
        "Script refused: an expression nested more than {0} levels deep ({1} in a function), the limit{2}",
        &[&MAX_EXPR_DEPTH, &MAX_FUNCTION_EXPR_DEPTH, &at(position)],
    )
}

fn is_limit(error: &EvalAltResult) -> bool {
    match error {
        EvalAltResult::ErrorTooManyOperations(_)
        | EvalAltResult::ErrorTerminated(..)
        | EvalAltResult::ErrorTooManyVariables(_)
        | EvalAltResult::ErrorStackOverflow(_)
        | EvalAltResult::ErrorDataTooLarge(..) => true,
        EvalAltResult::ErrorInFunctionCall(_, _, inner, _) => is_limit(inner),
        _ => false,
    }
}

fn at(position: Position) -> String {
    match (position.line(), position.position()) {
        (Some(line), Some(column)) => format!(
            " ({})",
            crate::i18n::tr("line {0}, position {1}", &[&line, &column])
        ),
        (Some(line), None) => format!(" ({})", crate::i18n::tr("line {0}", &[&line])),
        _ => String::new(),
    }
}

#[cfg(test)]
mod tests;
