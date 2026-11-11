// Rule scripts, written in Rhai (https://rhai.rs).
// Sandbox: a rule script comes from whoever can edit the configuration and runs on every matching request, so it
// gets a budget (script_budget.rs: operations, time, variables, call depth, size of each value) and no way out of the
// process:
// `import` cannot load files (Rhai's default resolver reads `.rhai` files from the disk), `eval` is disabled so that
// validating a script really validates all the code it runs, and `print`/`debug` go to the debug log instead of
// the server's stdout. The native functions registered below never panic, whatever their arguments.
// A rule has at most one script, run before its template is rendered; its result is available as
// {{script}} when it is a string, or {{script.field}} when it is a map.
//
// parse_json/to_json/parse_xml_items/xml_element exist for one need that neither template variables nor conditions
// can meet, because they cannot loop: a request that holds a list of objects and a response that must hold as many
// items, built position by position (JSON and XML/SOAP). docs/en/rhai-scripts.md has checked examples of both.
use std::collections::HashMap;
use std::sync::Arc;

#[derive(Clone)]
pub struct ScriptEngine {
    engine: Arc<rhai::Engine>,
}

#[derive(Debug, Clone, Default)]
pub struct ScriptResult {
    pub value: String,
    pub fields: HashMap<String, String>,
}

pub struct ScriptContext {
    pub body: String,
    pub headers: HashMap<String, String>,
    pub query_params: HashMap<String, String>,
    pub path_params: HashMap<String, String>,
}

impl Default for ScriptEngine {
    fn default() -> Self {
        Self::new()
    }
}

impl ScriptEngine {
    pub fn new() -> Self {
        let mut engine = rhai::Engine::new();
        super::script_budget::apply(&mut engine);
        engine.set_module_resolver(rhai::module_resolvers::DummyModuleResolver::new());
        engine.disable_symbol("eval");
        engine.on_print(|text| tracing::debug!(target: "mimicway::script", "print: {text}"));
        engine.on_debug(
            |text, _source, _pos| tracing::debug!(target: "mimicway::script", "debug: {text}"),
        );

        engine.register_fn("random_int", |min: i64, max: i64| -> i64 {
            if min >= max {
                return min;
            }
            fastrand::i64(min..=max)
        });

        engine.register_fn("now_ms", || -> i64 {
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis() as i64
        });

        engine.register_fn("now_iso", || -> String {
            crate::engine::template::epoch_to_iso(
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_secs(),
            )
        });

        engine.register_fn("year", || -> i64 {
            let secs = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs();
            let iso = crate::engine::template::epoch_to_iso(secs);
            iso[..4].parse().unwrap_or(2026)
        });

        // Formats: "iso" (default), "fr" (DD/MM/YYYY), "en" (MM/DD/YYYY). A negative or zero number of days counts as
        // today rather than silently moving the date the other way.
        engine.register_fn("date_now", || -> String { format_date_offset(0, "iso") });
        engine.register_fn("date_now", |format: &str| -> String {
            format_date_offset(0, format)
        });
        engine.register_fn("date_past", |days: i64| -> String {
            format_date_offset(-days.max(0), "iso")
        });
        engine.register_fn("date_past", |days: i64, format: &str| -> String {
            format_date_offset(-days.max(0), format)
        });
        engine.register_fn("date_future", |days: i64| -> String {
            format_date_offset(days.max(0), "iso")
        });
        engine.register_fn("date_future", |days: i64, format: &str| -> String {
            format_date_offset(days.max(0), format)
        });

        // The reverse of date_now: reads a date typed in an explicit pattern and returns milliseconds since the epoch. The
        // pattern is never guessed (03/04 is ambiguous). Fixed-width tokens (yyyy, MM, dd, HH, mm, ss; anything else is
        // literal) rather than strftime avoid a date formatting dependency. A text that does not fit, or a date that does
        // not exist (February 31), is a runtime error, never a silent wrong value.
        engine.register_fn(
            "parse_date",
            |text: &str, pattern: &str| -> Result<i64, Box<rhai::EvalAltResult>> {
                parse_date_impl(text, pattern)
            },
        );

        engine.register_fn("uuid", || -> String { uuid::Uuid::new_v4().to_string() });

        engine.register_fn("fake", |kind: &str| -> String {
            crate::engine::template::resolve_fake_public(kind)
        });

        // The same seed always gives the same value (one SIRET, one company name), through the FNV-1a hash already used for
        // group codes. The seed can be any Rhai value (string, integer, boolean...): its text form is hashed.
        engine.register_fn(
            "seeded_int",
            |seed: rhai::Dynamic, min: i64, max: i64| -> Result<i64, Box<rhai::EvalAltResult>> {
                Ok(seeded_int_impl(&seed_text(&seed)?, min, max))
            },
        );
        engine.register_fn(
            "seeded_pick",
            |seed: rhai::Dynamic,
             list: rhai::Array|
             -> Result<rhai::Dynamic, Box<rhai::EvalAltResult>> {
                Ok(seeded_pick_impl(&seed_text(&seed)?, &list))
            },
        );

        // JSON to navigable Rhai values (arrays, maps) and back. A script loops over `parse_json(request.body)` and returns,
        // for instance, `#{ items: to_json(out) }`, which the template inserts as text with {{script.items}}.
        engine.register_fn("parse_json", super::script_json::parse);
        engine.register_fn("to_json", |value: rhai::Dynamic| {
            super::script_json::to_text(&value)
        });

        // The XML counterparts. XML has no obvious mapping to arrays and maps, so instead of a generic conversion,
        // parse_xml_items() extracts the elements repeated at a path (what the need is: a list of items), one level of
        // children each, and xml_element() builds an element, recursively, from a Rhai value. Paths use the same syntax as
        // XPath conditions: segments separated by "/", namespace prefixes ignored.
        engine.register_fn("parse_xml_items", parse_xml_items_impl);
        engine.register_fn("xml_element", |tag: &str, value: rhai::Dynamic| {
            xml_element_impl(tag, &value)
        });

        Self {
            engine: Arc::new(engine),
        }
    }

    pub fn validate(&self, script: &str) -> Result<(), String> {
        self.engine
            .compile(script)
            .map(|_| ())
            .map_err(|e| super::script_budget::describe_parse(&e))
    }

    pub fn execute(&self, script: &str, context: &ScriptContext) -> Result<ScriptResult, String> {
        let mut scope = rhai::Scope::new();

        let mut request = rhai::Map::new();
        request.insert("body".into(), context.body.clone().into());

        let headers: rhai::Map = context
            .headers
            .iter()
            .map(|(k, v)| (k.as_str().into(), rhai::Dynamic::from(v.clone())))
            .collect();
        request.insert("headers".into(), rhai::Dynamic::from_map(headers));

        let query: rhai::Map = context
            .query_params
            .iter()
            .map(|(k, v)| (k.as_str().into(), rhai::Dynamic::from(v.clone())))
            .collect();
        request.insert("query".into(), rhai::Dynamic::from_map(query));

        let path: rhai::Map = context
            .path_params
            .iter()
            .map(|(k, v)| (k.as_str().into(), rhai::Dynamic::from(v.clone())))
            .collect();
        request.insert("path".into(), rhai::Dynamic::from_map(path));

        scope.push("request", rhai::Dynamic::from_map(request));

        // Compiled before the budget starts: the syntax tree is bounded by the length of the script, and the budget is
        // about what the run holds.
        let ast = self
            .engine
            .compile(script)
            .map_err(|e| super::script_budget::describe_parse(&e))?;
        let result = {
            let _clock = super::script_budget::RunClock::start();
            self.engine
                .eval_ast_with_scope::<rhai::Dynamic>(&mut scope, &ast)
        }
        .map_err(|e| super::script_budget::describe(&e))?;

        let describe = |e: Box<rhai::EvalAltResult>| super::script_budget::describe(&e);
        // Rhai checks a map's size when the map is read, not when an assignment adds a key: a map returned as it is
        // was never checked.
        self.engine
            .ensure_data_size_within_limits(&result)
            .map_err(describe)?;
        if result.is_map() {
            let map = result.cast::<rhai::Map>();
            let fields = map
                .into_iter()
                .map(|(k, v)| {
                    Ok((
                        k.to_string(),
                        dynamic_field_to_string(&v).map_err(describe)?,
                    ))
                })
                .collect::<Result<HashMap<String, String>, String>>()?;
            Ok(ScriptResult {
                value: String::new(),
                fields,
            })
        } else {
            Ok(ScriptResult {
                value: super::script_budget::display(&result).map_err(describe)?,
                fields: HashMap::new(),
            })
        }
    }
}

// Formats epoch_secs + days_delta days as "iso" (default), "fr" or "en", with the same civil calendar conversion as
// epoch_to_iso (Howard Hinnant's algorithm) instead of a date crate.
fn format_date_offset(days_delta: i64, format: &str) -> String {
    // About ±10,000 years: beyond, no calendar date exists and the arithmetic below would overflow.
    const MAX_DAYS: i64 = 3_652_425;
    let days_delta = days_delta.clamp(-MAX_DAYS, MAX_DAYS);
    let now_secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64;
    let days = (now_secs + days_delta * 86400).div_euclid(86400);
    let (y, mo, d) = crate::engine::template::civil_from_days(days);
    match format {
        "fr" => format!("{d:02}/{mo:02}/{y:04}"),
        "en" => format!("{mo:02}/{d:02}/{y:04}"),
        _ => format!("{y:04}-{mo:02}-{d:02}"),
    }
}

// Reads `text` with `pattern` and returns milliseconds since the epoch, in UTC like now_ms() and epoch_to_iso().
// Each token's width is the number of digits read ("yyyy" four, "dd" two): no variable width, so a pattern reads the
// same way the fixed-width output formats write. yyyy, MM and dd are required; HH, mm and ss default to 00:00:00.
fn parse_date_impl(text: &str, pattern: &str) -> Result<i64, Box<rhai::EvalAltResult>> {
    let text_chars: Vec<char> = text.chars().collect();
    let pattern_chars: Vec<char> = pattern.chars().collect();

    let mut year: Option<i64> = None;
    let mut month: Option<u32> = None;
    let mut day: Option<u32> = None;
    let mut hour: u32 = 0;
    let mut minute: u32 = 0;
    let mut second: u32 = 0;

    let mut pi = 0usize;
    let mut ti = 0usize;

    while pi < pattern_chars.len() {
        let token = pattern_chars[pi];
        if matches!(token, 'y' | 'M' | 'd' | 'H' | 'm' | 's') {
            let start = pi;
            while pi < pattern_chars.len() && pattern_chars[pi] == token {
                pi += 1;
            }
            let width = pi - start;
            if ti + width > text_chars.len() {
                return Err(crate::i18n::tr(
                    "parse_date: '{0}' is too short for the pattern '{1}' ({2} digit(s) missing for the field '{3}')",
                    &[&text, &pattern, &(ti + width - text_chars.len()), &token.to_string().repeat(width)],
                )
                .into());
            }
            let slice: String = text_chars[ti..ti + width].iter().collect();
            if !slice.chars().all(|c| c.is_ascii_digit()) {
                return Err(crate::i18n::tr(
                    "parse_date: '{0}' is not a valid number for the field '{1}' of the pattern '{2}' (in '{3}')",
                    &[&slice, &token.to_string().repeat(width), &pattern, &text],
                )
                .into());
            }
            let value: i64 = slice.parse().unwrap_or_default();
            match token {
                'y' => year = Some(value),
                'M' => month = Some(value as u32),
                'd' => day = Some(value as u32),
                'H' => hour = value as u32,
                'm' => minute = value as u32,
                's' => second = value as u32,
                _ => unreachable!(),
            }
            ti += width;
        } else {
            if ti >= text_chars.len() || text_chars[ti] != token {
                return Err(crate::i18n::tr(
                    "parse_date: character '{0}' expected at position {1} of '{2}' for the pattern '{3}'",
                    &[&token, &ti, &text, &pattern],
                )
                .into());
            }
            pi += 1;
            ti += 1;
        }
    }

    if ti != text_chars.len() {
        return Err(crate::i18n::tr(
            "parse_date: '{0}' has extra characters after the pattern '{1}'",
            &[&text, &pattern],
        )
        .into());
    }

    let year = year.ok_or_else(|| {
        crate::i18n::tr(
            "parse_date: the pattern '{0}' has no year (yyyy)",
            &[&pattern],
        )
    })?;
    if !(0..=9999).contains(&year) {
        return Err(crate::i18n::tr(
            "parse_date: year {0} in '{1}' is out of range (0000 to 9999)",
            &[&year, &text],
        )
        .into());
    }
    let month = month.ok_or_else(|| {
        crate::i18n::tr(
            "parse_date: the pattern '{0}' has no month (MM)",
            &[&pattern],
        )
    })?;
    let day = day.ok_or_else(|| {
        crate::i18n::tr("parse_date: the pattern '{0}' has no day (dd)", &[&pattern])
    })?;

    if !(1..=12).contains(&month) {
        return Err(crate::i18n::tr(
            "parse_date: invalid month {0} in '{1}' (must be between 01 and 12)",
            &[&format!("{month:02}"), &text],
        )
        .into());
    }
    if !(1..=31).contains(&day) {
        return Err(crate::i18n::tr(
            "parse_date: invalid day {0} in '{1}' (must be between 01 and 31)",
            &[&format!("{day:02}"), &text],
        )
        .into());
    }
    if hour > 23 {
        return Err(crate::i18n::tr(
            "parse_date: invalid hour {0} in '{1}' (must be between 00 and 23)",
            &[&format!("{hour:02}"), &text],
        )
        .into());
    }
    if minute > 59 {
        return Err(crate::i18n::tr(
            "parse_date: invalid minute {0} in '{1}' (must be between 00 and 59)",
            &[&format!("{minute:02}"), &text],
        )
        .into());
    }
    if second > 59 {
        return Err(crate::i18n::tr(
            "parse_date: invalid second {0} in '{1}' (must be between 00 and 59)",
            &[&format!("{second:02}"), &text],
        )
        .into());
    }

    let days = crate::engine::template::days_from_civil(year, month, day);
    // Converting the day count back with civil_from_days catches dates that do not exist (February 31) without
    // repeating the month length and leap year rules: such a date does not come back as the same triple.
    if crate::engine::template::civil_from_days(days) != (year, month, day) {
        return Err(crate::i18n::tr(
            "parse_date: '{0}' is not a valid date (no such day in that month)",
            &[&format!("{day:02}/{month:02}/{year:04}")],
        )
        .into());
    }

    let total_seconds = days * 86400 + hour as i64 * 3600 + minute as i64 * 60 + second as i64;
    Ok(total_seconds * 1000)
}

fn seeded_int_impl(seed: &str, min: i64, max: i64) -> i64 {
    if min >= max {
        return min;
    }
    let hash = crate::server::codegen::fnv1a_hash(seed);
    // i128: `max - min + 1` overflows i64 (and is 0 once wrapped) for the widest ranges.
    let range = (max as i128 - min as i128 + 1) as u128;
    (min as i128 + (hash as u128 % range) as i128) as i64
}

// A text seed is hashed as it is, without a copy (a request body may be one); any other value through its text form,
// which stops at the text limit.
fn seed_text(seed: &rhai::Dynamic) -> Result<rhai::ImmutableString, Box<rhai::EvalAltResult>> {
    if seed.is_string()
        && let Ok(text) = seed.clone().into_immutable_string()
    {
        return Ok(text);
    }
    super::script_budget::display(seed).map(Into::into)
}

fn seeded_pick_impl(seed: &str, list: &rhai::Array) -> rhai::Dynamic {
    if list.is_empty() {
        return rhai::Dynamic::UNIT;
    }
    let hash = crate::server::codegen::fnv1a_hash(seed);
    let idx = (hash % list.len() as u64) as usize;
    list[idx].clone()
}

// Text form of a field of the map a script returns, for {{script.field}} (one flat key, never a nested path). A
// scalar keeps its plain text. A map or an array (a picked object nested under a key, for instance
// `#{ city: pick, id: uuid() }`) becomes JSON, because Rhai's own display form (`#{"k": "v"}`) is neither JSON
// nor XML once pasted into a response.
fn dynamic_field_to_string(value: &rhai::Dynamic) -> Result<String, Box<rhai::EvalAltResult>> {
    if value.is_map() || value.is_array() {
        super::script_json::to_text(value)
    } else {
        super::script_budget::display(value)
    }
}

// --- XML: extracting repeated elements, building elements (parse_xml_items, xml_element) ---

// Extracts every element repeated at `path` as an array of maps: one entry per child element, keyed by its tag, valued
// by its text. The path reads as in XPath conditions: segments separated by "/", from the root element down, each one
// a child of the one before, the last one being the repeated tag, namespace prefixes ignored. Only the first level of
// children is read, which fits flat SOAP items such as `<product><sku>A</sku><qty>2</qty></product>`; deeper structure
// inside an item is ignored, since a generic XML tree would be out of proportion with this need. A self-closing
// element is the same as an empty one (`<qty/>` gives an empty text, `<product/>` an empty item). Extraction stops
// at the first item, field or byte of text over the limits of a value (field names count with the text), rather than
// gathering a large body whole before Rhai checks it, and at the first element nested deeper than `XML_DEPTH_LIMIT`;
// it stops at the first malformed part of the text, keeping the items read before it.
fn parse_xml_items_impl(xml: &str, path: &str) -> Result<rhai::Array, Box<rhai::EvalAltResult>> {
    use super::script_budget::{
        ARRAY_TOO_LARGE, MAP_TOO_LARGE, MAX_ARRAY_SIZE, MAX_MAP_SIZE, MAX_STRING_SIZE,
        TEXT_TOO_LARGE, XML_TOO_DEEP, too_large,
    };
    use crate::engine::matcher::XML_DEPTH_LIMIT;
    use quick_xml::events::Event;
    use quick_xml::reader::Reader;

    let segments: Vec<&str> = path.split('/').filter(|s| !s.is_empty()).collect();
    let Some((item_tag, parent_segments)) = segments.split_last() else {
        return Ok(rhai::Array::new());
    };

    let mut reader = Reader::from_str(xml);
    // Outside the items, two counters stand for the stack of open elements, whatever its depth: `depth` elements are
    // open, and the first `on_path` of them, from the root, are the first parents of the path.
    let (mut depth, mut on_path) = (0usize, 0usize);
    let mut items = rhai::Array::new();

    // item_depth: 0 outside an item, 1 inside the item element, 2 inside one of its children (the captured text).
    let mut item_depth: u32 = 0;
    let mut current_item = rhai::Map::new();
    let mut current_field = String::new();
    let mut current_text = String::new();
    let (mut fields, mut text_bytes) = (0usize, 0usize);

    loop {
        // A self-closing element is read as its start followed by its end.
        let (opened, closes) = match reader.read_event() {
            Ok(Event::Start(e)) => {
                // `depth + item_depth` elements are open: the parents outside the items, and the item with its
                // open children inside one. A self-closing element below counts the same.
                if depth + item_depth as usize >= XML_DEPTH_LIMIT {
                    return Err(too_large(XML_TOO_DEEP));
                }
                (
                    Some(crate::engine::matcher::MatchEngine::local_name(&e)),
                    false,
                )
            }
            Ok(Event::Empty(e)) => {
                if depth + item_depth as usize >= XML_DEPTH_LIMIT {
                    return Err(too_large(XML_TOO_DEEP));
                }
                (
                    Some(crate::engine::matcher::MatchEngine::local_name(&e)),
                    true,
                )
            }
            Ok(Event::End(_)) => (None, true),
            Ok(event @ (Event::Text(_) | Event::GeneralRef(_))) => {
                if item_depth == 2
                    && let Some(text) = crate::engine::matcher::MatchEngine::xml_text(&event)
                {
                    if text_bytes + current_field.len() + current_text.len() + text.len()
                        > MAX_STRING_SIZE
                    {
                        return Err(too_large(TEXT_TOO_LARGE));
                    }
                    current_text.push_str(&text);
                }
                continue;
            }
            Ok(Event::Eof) | Err(_) => break,
            Ok(_) => continue,
        };
        if let Some(local) = opened {
            if item_depth == 0 {
                if on_path == parent_segments.len() && depth == on_path && local == *item_tag {
                    item_depth = 1;
                    current_item = rhai::Map::new();
                } else {
                    if on_path == depth
                        && depth < parent_segments.len()
                        && local == parent_segments[depth]
                    {
                        on_path += 1;
                    }
                    depth += 1;
                }
            } else if item_depth == 1 {
                current_field = local;
                current_text.clear();
                item_depth = 2;
            } else {
                item_depth += 1;
            }
        }
        if closes {
            match item_depth {
                0 => {
                    // The reader, as configured by default (`allow_unmatched_ends` off), refuses an end tag that
                    // closes no open element: `depth` is never 0 here.
                    depth -= 1;
                    on_path = on_path.min(depth);
                }
                1 => {
                    if items.len() == MAX_ARRAY_SIZE {
                        return Err(too_large(ARRAY_TOO_LARGE));
                    }
                    items.push(rhai::Dynamic::from_map(std::mem::take(&mut current_item)));
                    item_depth = 0;
                }
                2 => {
                    fields += 1;
                    if fields > MAX_MAP_SIZE {
                        return Err(too_large(MAP_TOO_LARGE));
                    }
                    // A field without text (`<qty/>`, `<qty></qty>`) brings its name alone, checked here.
                    text_bytes += current_field.len() + current_text.trim().len();
                    if text_bytes > MAX_STRING_SIZE {
                        return Err(too_large(TEXT_TOO_LARGE));
                    }
                    current_item.insert(
                        std::mem::take(&mut current_field).into(),
                        rhai::Dynamic::from(current_text.trim().to_string()),
                    );
                    current_text.clear();
                    item_depth = 1;
                }
                _ => item_depth -= 1,
            }
        }
    }
    Ok(items)
}

// Builds `<tag>...</tag>` from a Rhai value:
// - a map gives one child element per key, recursively; an array under a key repeats that key's tag once per item
//   (XML has no array syntax);
// - an array at the top repeats `tag` once per item (handy to build a list in one call);
// - a scalar gives escaped text;
// - unit (missing value) gives `<tag/>`.
fn xml_element_impl(tag: &str, value: &rhai::Dynamic) -> Result<String, Box<rhai::EvalAltResult>> {
    let mut out = String::new();
    write_xml_element(&mut out, tag, value)?;
    Ok(out)
}

// Every piece goes through `push_xml`, which refuses to grow the text past its limit: a tag repeated once per item
// would otherwise build gigabytes (a long tag over a thousand items) before Rhai checks the result. Arrays and maps
// are read in place, never copied.
fn write_xml_element(
    out: &mut String,
    tag: &str,
    value: &rhai::Dynamic,
) -> Result<(), Box<rhai::EvalAltResult>> {
    if let Some(map) = value.read_lock::<rhai::Map>() {
        push_xml(out, &["<", tag, ">"])?;
        for (key, item) in map.iter() {
            match item.read_lock::<rhai::Array>() {
                Some(array) => {
                    for entry in array.iter() {
                        write_xml_element(out, key, entry)?;
                    }
                }
                None => write_xml_element(out, key, item)?,
            }
        }
        push_xml(out, &["</", tag, ">"])
    } else if let Some(array) = value.read_lock::<rhai::Array>() {
        for item in array.iter() {
            write_xml_element(out, tag, item)?;
        }
        Ok(())
    } else if value.is_unit() {
        push_xml(out, &["<", tag, "/>"])
    } else {
        push_xml(out, &["<", tag, ">"])?;
        match value.read_lock::<rhai::ImmutableString>() {
            Some(text) => push_xml_text(out, &text)?,
            None => push_xml_text(out, &value.to_string())?,
        }
        push_xml(out, &["</", tag, ">"])
    }
}

fn push_xml(out: &mut String, parts: &[&str]) -> Result<(), Box<rhai::EvalAltResult>> {
    let length: usize = parts.iter().map(|part| part.len()).sum();
    if out.len() + length > super::script_budget::MAX_STRING_SIZE {
        return Err(super::script_budget::too_large(
            super::script_budget::TEXT_TOO_LARGE,
        ));
    }
    parts.iter().for_each(|part| out.push_str(part));
    Ok(())
}

fn push_xml_text(out: &mut String, text: &str) -> Result<(), Box<rhai::EvalAltResult>> {
    for c in text.chars() {
        let mut buffer = [0; 4];
        let piece = match c {
            '&' => "&amp;",
            '<' => "&lt;",
            '>' => "&gt;",
            '"' => "&quot;",
            '\'' => "&apos;",
            _ => c.encode_utf8(&mut buffer),
        };
        push_xml(out, &[piece])?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn empty_ctx() -> ScriptContext {
        ScriptContext {
            body: String::new(),
            headers: HashMap::new(),
            query_params: HashMap::new(),
            path_params: HashMap::new(),
        }
    }

    #[test]
    fn simple_string_result() {
        let engine = ScriptEngine::new();
        let result = engine.execute(r#""hello""#, &empty_ctx()).unwrap();
        assert_eq!(result.value, "hello");
        assert!(result.fields.is_empty());
    }

    #[test]
    fn map_result_returns_fields() {
        let engine = ScriptEngine::new();
        let result = engine
            .execute(r#"#{ greeting: "hi", count: "42" }"#, &empty_ctx())
            .unwrap();
        assert_eq!(result.fields.get("greeting").unwrap(), "hi");
        assert_eq!(result.fields.get("count").unwrap(), "42");
    }

    #[test]
    fn access_request_context() {
        let engine = ScriptEngine::new();
        let mut path = HashMap::new();
        path.insert("id".into(), "123".into());
        let ctx = ScriptContext {
            path_params: path,
            ..empty_ctx()
        };
        let result = engine.execute("request.path.id", &ctx).unwrap();
        assert_eq!(result.value, "123");
    }

    #[test]
    fn string_manipulation() {
        let engine = ScriptEngine::new();
        let result = engine
            .execute(r#"let x = "hello"; x.to_upper()"#, &empty_ctx())
            .unwrap();
        assert_eq!(result.value, "HELLO");
    }

    #[test]
    fn invalid_script_returns_error() {
        let engine = ScriptEngine::new();
        let result = engine.execute("this is not valid", &empty_ctx());
        assert!(result.is_err());
    }

    #[test]
    fn access_query_params() {
        let engine = ScriptEngine::new();
        let mut query = HashMap::new();
        query.insert("page".into(), "5".into());
        let ctx = ScriptContext {
            query_params: query,
            ..empty_ctx()
        };
        let result = engine.execute("request.query.page", &ctx).unwrap();
        assert_eq!(result.value, "5");
    }

    #[test]
    fn random_int_in_range() {
        let engine = ScriptEngine::new();
        for _ in 0..20 {
            let result = engine.execute("random_int(1, 5)", &empty_ctx()).unwrap();
            let val: i64 = result.value.parse().unwrap();
            assert!((1..=5).contains(&val), "random_int(1,5) returned {val}");
        }
    }

    #[test]
    fn now_ms_returns_timestamp() {
        let engine = ScriptEngine::new();
        let result = engine.execute("now_ms()", &empty_ctx()).unwrap();
        let val: i64 = result.value.parse().unwrap();
        assert!(val > 1_700_000_000_000, "now_ms too small: {val}");
    }

    #[test]
    fn ratio_script_returns_map() {
        let engine = ScriptEngine::new();
        let script = r#"
            let roll = random_int(1, 5);
            if roll <= 4 {
                #{ status: "actif", code: "200" }
            } else {
                #{ status: "suspendu", code: "403" }
            }
        "#;
        let result = engine.execute(script, &empty_ctx()).unwrap();
        let status = result.fields.get("status").unwrap();
        assert!(status == "actif" || status == "suspendu");
    }

    #[test]
    fn year_returns_4_digits() {
        let engine = ScriptEngine::new();
        let result = engine.execute("year()", &empty_ctx()).unwrap();
        let y: i64 = result.value.parse().unwrap();
        assert!((2025..=2100).contains(&y));
    }

    #[test]
    fn date_now_returns_iso_date_by_default() {
        let engine = ScriptEngine::new();
        let result = engine.execute("date_now()", &empty_ctx()).unwrap();
        assert_eq!(result.value.len(), 10);
        assert!(result.value.contains('-'));
    }

    #[test]
    fn date_now_explicit_iso_matches_default() {
        let engine = ScriptEngine::new();
        let default = engine.execute("date_now()", &empty_ctx()).unwrap().value;
        let explicit = engine
            .execute(r#"date_now("iso")"#, &empty_ctx())
            .unwrap()
            .value;
        assert_eq!(default, explicit);
    }

    #[test]
    fn date_now_fr_format() {
        let engine = ScriptEngine::new();
        let iso = engine.execute("date_now()", &empty_ctx()).unwrap().value;
        let fr = engine
            .execute(r#"date_now("fr")"#, &empty_ctx())
            .unwrap()
            .value;
        let parts: Vec<&str> = iso.split('-').collect();
        assert_eq!(fr, format!("{}/{}/{}", parts[2], parts[1], parts[0]));
    }

    #[test]
    fn date_now_en_format() {
        let engine = ScriptEngine::new();
        let iso = engine.execute("date_now()", &empty_ctx()).unwrap().value;
        let en = engine
            .execute(r#"date_now("en")"#, &empty_ctx())
            .unwrap()
            .value;
        let parts: Vec<&str> = iso.split('-').collect();
        assert_eq!(en, format!("{}/{}/{}", parts[1], parts[2], parts[0]));
    }

    #[test]
    fn date_now_unknown_format_falls_back_to_iso() {
        let engine = ScriptEngine::new();
        let iso = engine.execute("date_now()", &empty_ctx()).unwrap().value;
        let unknown = engine
            .execute(r#"date_now("klingon")"#, &empty_ctx())
            .unwrap()
            .value;
        assert_eq!(iso, unknown);
    }

    #[test]
    fn date_past_is_before_today() {
        let engine = ScriptEngine::new();
        let today = engine.execute("date_now()", &empty_ctx()).unwrap().value;
        let past = engine.execute("date_past(10)", &empty_ctx()).unwrap().value;
        assert!(past < today);
    }

    #[test]
    fn date_future_is_after_today() {
        let engine = ScriptEngine::new();
        let today = engine.execute("date_now()", &empty_ctx()).unwrap().value;
        let future = engine
            .execute("date_future(10)", &empty_ctx())
            .unwrap()
            .value;
        assert!(future > today);
    }

    #[test]
    fn date_past_zero_days_is_today() {
        let engine = ScriptEngine::new();
        let today = engine.execute("date_now()", &empty_ctx()).unwrap().value;
        let past = engine.execute("date_past(0)", &empty_ctx()).unwrap().value;
        assert_eq!(past, today);
    }

    #[test]
    fn date_past_negative_days_clamped_to_today() {
        let engine = ScriptEngine::new();
        let today = engine.execute("date_now()", &empty_ctx()).unwrap().value;
        let past = engine
            .execute("date_past(-30)", &empty_ctx())
            .unwrap()
            .value;
        assert_eq!(past, today);
    }

    #[test]
    fn date_future_zero_days_is_today() {
        let engine = ScriptEngine::new();
        let today = engine.execute("date_now()", &empty_ctx()).unwrap().value;
        let future = engine
            .execute("date_future(0)", &empty_ctx())
            .unwrap()
            .value;
        assert_eq!(future, today);
    }

    #[test]
    fn date_future_negative_days_clamped_to_today() {
        let engine = ScriptEngine::new();
        let today = engine.execute("date_now()", &empty_ctx()).unwrap().value;
        let future = engine
            .execute("date_future(-30)", &empty_ctx())
            .unwrap()
            .value;
        assert_eq!(future, today);
    }

    #[test]
    fn date_past_with_format() {
        let engine = ScriptEngine::new();
        let result = engine
            .execute(r#"date_past(5, "fr")"#, &empty_ctx())
            .unwrap()
            .value;
        assert_eq!(result.len(), 10);
        assert_eq!(result.chars().filter(|c| *c == '/').count(), 2);
    }

    #[test]
    fn date_future_with_format() {
        let engine = ScriptEngine::new();
        let result = engine
            .execute(r#"date_future(5, "en")"#, &empty_ctx())
            .unwrap()
            .value;
        assert_eq!(result.len(), 10);
        assert_eq!(result.chars().filter(|c| *c == '/').count(), 2);
    }

    #[test]
    fn parse_date_iso_pattern_date_only() {
        let engine = ScriptEngine::new();
        let result = engine
            .execute(r#"parse_date("2026-03-15", "yyyy-MM-dd")"#, &empty_ctx())
            .unwrap()
            .value;
        assert_eq!(result, "1773532800000");
    }

    #[test]
    fn parse_date_fr_pattern_date_only() {
        let engine = ScriptEngine::new();
        let result = engine
            .execute(r#"parse_date("15/03/2026", "dd/MM/yyyy")"#, &empty_ctx())
            .unwrap()
            .value;
        assert_eq!(result, "1773532800000");
    }

    #[test]
    fn parse_date_fr_pattern_with_time() {
        let engine = ScriptEngine::new();
        let result = engine
            .execute(
                r#"parse_date("15/03/2026 08:30:45", "dd/MM/yyyy HH:mm:ss")"#,
                &empty_ctx(),
            )
            .unwrap()
            .value;
        // 1773532800000 (midnight) plus 8 h 30 min 45 s, in milliseconds
        assert_eq!(result, "1773563445000");
    }

    #[test]
    fn parse_date_without_time_defaults_to_midnight() {
        let engine = ScriptEngine::new();
        let date_only = engine
            .execute(r#"parse_date("2026-03-15", "yyyy-MM-dd")"#, &empty_ctx())
            .unwrap()
            .value;
        let with_midnight = engine
            .execute(
                r#"parse_date("2026-03-15 00:00:00", "yyyy-MM-dd HH:mm:ss")"#,
                &empty_ctx(),
            )
            .unwrap()
            .value;
        assert_eq!(date_only, with_midnight);
    }

    #[test]
    fn parse_date_leap_year_feb_29_is_valid() {
        let engine = ScriptEngine::new();
        let result = engine.execute(r#"parse_date("29/02/2028", "dd/MM/yyyy")"#, &empty_ctx());
        assert!(
            result.is_ok(),
            "29/02/2028 must be valid (2028 is a leap year)"
        );
    }

    #[test]
    fn parse_date_non_leap_year_feb_29_is_invalid() {
        let engine = ScriptEngine::new();
        let result = engine.execute(r#"parse_date("29/02/2026", "dd/MM/yyyy")"#, &empty_ctx());
        assert!(
            result.is_err(),
            "29/02/2026 must be refused (2026 is not a leap year)"
        );
    }

    #[test]
    fn parse_date_feb_31_is_invalid() {
        let engine = ScriptEngine::new();
        let result = engine.execute(r#"parse_date("31/02/2026", "dd/MM/yyyy")"#, &empty_ctx());
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(
            err.contains("not a valid date"),
            "message d'erreur inattendu : {err}"
        );
    }

    #[test]
    fn parse_date_month_out_of_range_is_invalid() {
        let engine = ScriptEngine::new();
        let result = engine.execute(r#"parse_date("15/13/2026", "dd/MM/yyyy")"#, &empty_ctx());
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("invalid month"));
    }

    #[test]
    fn parse_date_hour_out_of_range_is_invalid() {
        let engine = ScriptEngine::new();
        let result = engine.execute(
            r#"parse_date("15/03/2026 25:00:00", "dd/MM/yyyy HH:mm:ss")"#,
            &empty_ctx(),
        );
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("invalid hour"));
    }

    #[test]
    fn parse_date_non_numeric_field_is_invalid() {
        let engine = ScriptEngine::new();
        let result = engine.execute(r#"parse_date("ab/03/2026", "dd/MM/yyyy")"#, &empty_ctx());
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("is not a valid number"));
    }

    #[test]
    fn parse_date_literal_separator_mismatch_is_invalid() {
        let engine = ScriptEngine::new();
        // The pattern expects "/" but the text uses "-"
        let result = engine.execute(r#"parse_date("15-03-2026", "dd/MM/yyyy")"#, &empty_ctx());
        assert!(result.is_err());
    }

    #[test]
    fn parse_date_text_too_short_is_invalid() {
        let engine = ScriptEngine::new();
        let result = engine.execute(r#"parse_date("15/03/26", "dd/MM/yyyy")"#, &empty_ctx());
        assert!(result.is_err());
        assert!(result.unwrap_err().contains(
            "too short for the pattern 'dd/MM/yyyy' (2 digit(s) missing for the field 'yyyy')"
        ));
    }

    /// Days from 1970-01-01, counted year by year and month by month from a table of month lengths.
    fn counted_days(year: i64, month: u32, day: u32) -> i64 {
        let leap = |y: i64| (y % 4 == 0 && y % 100 != 0) || y % 400 == 0;
        let year_length = |y: i64| if leap(y) { 366 } else { 365 };
        let mut days: i64 = if year >= 1970 {
            (1970..year).map(year_length).sum()
        } else {
            -(year..1970).map(year_length).sum::<i64>()
        };
        days += (1..month)
            .map(|m| month_length(year, m) as i64)
            .sum::<i64>();
        days + day as i64 - 1
    }

    fn month_length(year: i64, month: u32) -> u32 {
        let leap = (year % 4 == 0 && year % 100 != 0) || year % 400 == 0;
        [
            31,
            if leap { 29 } else { 28 },
            31,
            30,
            31,
            30,
            31,
            31,
            30,
            31,
            30,
            31,
        ][month as usize - 1]
    }

    // Each field is drawn around its limits (one under, at, one over) or anywhere in a range a little wider than its
    // valid one; a date is valid when every field is, by the table of month lengths, and gives its milliseconds.
    #[test]
    fn parse_date_accepts_a_date_exactly_when_each_field_is_valid() {
        use proptest::prelude::*;
        use proptest::test_runner::{Config, RngSeed, TestRunner};
        let config = Config {
            cases: 1024,
            rng_seed: RngSeed::Fixed(0x7061_7273_655f_6474),
            failure_persistence: None,
            ..Config::default()
        };
        let around = |limits: Vec<u32>, wide: std::ops::RangeInclusive<u32>| {
            prop_oneof![prop::sample::select(limits), wide]
        };
        let strategy = (
            prop_oneof![
                prop::sample::select(vec![0i64, 1900, 1969, 1970, 2000, 2028, 9999]),
                0..=9999i64
            ],
            around(vec![0, 1, 2, 12, 13], 0..=13),
            around(vec![0, 1, 28, 29, 30, 31, 32], 0..=32),
            around(vec![0, 23, 24], 0..=25),
            around(vec![0, 59, 60], 0..=61),
            around(vec![0, 59, 60], 0..=61),
        );
        let result = TestRunner::new(config).run(&strategy, |(y, mo, d, h, mi, s)| {
            let text = format!("{y:04}-{mo:02}-{d:02} {h:02}:{mi:02}:{s:02}");
            let valid = (1..=12).contains(&mo)
                && d >= 1
                && d <= month_length(y, mo)
                && h <= 23
                && mi <= 59
                && s <= 59;
            let parsed = parse_date_impl(&text, "yyyy-MM-dd HH:mm:ss").ok();
            let expected = valid.then(|| {
                (counted_days(y, mo, d) * 86_400 + h as i64 * 3_600 + mi as i64 * 60 + s as i64)
                    * 1_000
            });
            prop_assert_eq!(parsed, expected, "{}", text);
            Ok(())
        });
        if let Err(failure) = result {
            panic!("{failure}");
        }
    }

    #[test]
    fn parse_date_text_too_long_is_invalid() {
        let engine = ScriptEngine::new();
        let result = engine.execute(
            r#"parse_date("15/03/2026extra", "dd/MM/yyyy")"#,
            &empty_ctx(),
        );
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("extra characters"));
    }

    #[test]
    fn parse_date_error_is_visible_not_silent() {
        // A date that does not exist is a real runtime error (Err), never an empty value as a missing map key would give.
        let engine = ScriptEngine::new();
        let result = engine.execute(r#"parse_date("31/02/2026", "dd/MM/yyyy")"#, &empty_ctx());
        assert!(result.is_err());
    }

    #[test]
    fn seeded_int_is_deterministic_for_same_seed() {
        let engine = ScriptEngine::new();
        let a = engine
            .execute(r#"seeded_int("44306184100047", 0, 1000)"#, &empty_ctx())
            .unwrap()
            .value;
        let b = engine
            .execute(r#"seeded_int("44306184100047", 0, 1000)"#, &empty_ctx())
            .unwrap()
            .value;
        assert_eq!(a, b);
    }

    #[test]
    fn seeded_int_respects_bounds() {
        let engine = ScriptEngine::new();
        for seed in ["a", "b", "c", "siret-123", "0", "-1"] {
            let script = format!(r#"seeded_int("{seed}", 10, 20)"#);
            let result = engine.execute(&script, &empty_ctx()).unwrap();
            let val: i64 = result.value.parse().unwrap();
            assert!((10..=20).contains(&val), "seeded_int out of bounds: {val}");
        }
    }

    // Over 64 seeds, a range of a few values gives each of them, and nothing outside, wherever the range lies; a
    // range of one value gives that value.
    #[test]
    fn seeded_int_reaches_every_value_of_its_range() {
        for (min, max) in [
            (0, 1),
            (0, 2),
            (-3, 2),
            (i64::MIN, i64::MIN + 3),
            (i64::MAX - 4, i64::MAX),
            (7, 7),
        ] {
            let values: std::collections::BTreeSet<i64> = (0..64)
                .map(|i| seeded_int_impl(&format!("seed-{i}"), min, max))
                .collect();
            assert_eq!(values, (min..=max).collect(), "{min}..={max}");
        }
    }

    #[test]
    fn seeded_int_different_seeds_can_differ() {
        let engine = ScriptEngine::new();
        let values: Vec<String> = (0..10)
            .map(|i| {
                let script = format!(r#"seeded_int("seed-{i}", 0, 1000000)"#);
                engine.execute(&script, &empty_ctx()).unwrap().value
            })
            .collect();
        let unique: std::collections::HashSet<_> = values.iter().collect();
        assert!(
            unique.len() > 1,
            "expected different seeds to produce at least some different values"
        );
    }

    #[test]
    fn seeded_pick_is_deterministic_for_same_seed() {
        let engine = ScriptEngine::new();
        let script =
            r#"seeded_pick(request.path.siret, ["Dupont SARL", "Martin SAS", "Petit EURL"])"#;
        let mut path = HashMap::new();
        path.insert("siret".into(), "44306184100047".into());
        let ctx = ScriptContext {
            path_params: path,
            ..empty_ctx()
        };
        let a = engine.execute(script, &ctx).unwrap().value;
        let b = engine.execute(script, &ctx).unwrap().value;
        assert_eq!(a, b);
        assert!(["Dupont SARL", "Martin SAS", "Petit EURL"].contains(&a.as_str()));
    }

    #[test]
    fn seeded_pick_different_seed_can_pick_different_element() {
        let engine = ScriptEngine::new();
        let script = |siret: &str| -> String {
            let mut path = HashMap::new();
            path.insert("siret".into(), siret.into());
            let ctx = ScriptContext {
                path_params: path,
                ..empty_ctx()
            };
            engine
                .execute(
                    r#"seeded_pick(request.path.siret, ["Dupont SARL", "Martin SAS", "Petit EURL", "Durand SA", "Leroy SCI"])"#,
                    &ctx,
                )
                .unwrap()
                .value
        };
        let picks: std::collections::HashSet<String> =
            (0..10).map(|i| script(&format!("siret-{i}"))).collect();
        assert!(
            picks.len() > 1,
            "expected different SIRET values to yield at least some different picks"
        );
    }

    #[test]
    fn uuid_format() {
        let engine = ScriptEngine::new();
        let result = engine.execute("uuid()", &empty_ctx()).unwrap();
        assert_eq!(result.value.len(), 36);
        assert_eq!(result.value.chars().filter(|c| *c == '-').count(), 4);
    }

    #[test]
    fn fake_returns_data() {
        let engine = ScriptEngine::new();
        let result = engine
            .execute(r#"fake("FirstName")"#, &empty_ctx())
            .unwrap();
        assert!(!result.value.is_empty());
    }

    #[test]
    fn compose_date_with_string() {
        let engine = ScriptEngine::new();
        let result = engine
            .execute(r#"`${year()}-05-10`"#, &empty_ctx())
            .unwrap();
        assert!(result.value.ends_with("-05-10"));
        assert_eq!(result.value.len(), 10);
    }

    // --- parse_json / to_json ---

    #[test]
    fn parse_json_array_len_and_index_access() {
        let engine = ScriptEngine::new();
        let ctx = ScriptContext {
            body: r#"[{"id":"1"},{"id":"2"},{"id":"3"}]"#.into(),
            ..empty_ctx()
        };
        let result = engine
            .execute("parse_json(request.body).len()", &ctx)
            .unwrap();
        assert_eq!(result.value, "3");
        let result = engine
            .execute("parse_json(request.body)[1].id", &ctx)
            .unwrap();
        assert_eq!(result.value, "2");
    }

    #[test]
    fn parse_json_object_field_access() {
        let engine = ScriptEngine::new();
        let ctx = ScriptContext {
            body: r#"{"name":"Alice","age":30}"#.into(),
            ..empty_ctx()
        };
        let result = engine
            .execute("parse_json(request.body).name", &ctx)
            .unwrap();
        assert_eq!(result.value, "Alice");
        let result = engine
            .execute("parse_json(request.body).age", &ctx)
            .unwrap();
        assert_eq!(result.value, "30");
    }

    #[test]
    fn parse_json_invalid_returns_unit_not_error() {
        let engine = ScriptEngine::new();
        let ctx = ScriptContext {
            body: "not valid json".into(),
            ..empty_ctx()
        };
        let result = engine
            .execute("type_of(parse_json(request.body))", &ctx)
            .unwrap();
        assert_eq!(result.value, "()");
    }

    #[test]
    fn to_json_scalar_and_map() {
        let engine = ScriptEngine::new();
        let result = engine.execute(r#"to_json("hello")"#, &empty_ctx()).unwrap();
        assert_eq!(result.value, "\"hello\"");
        let result = engine
            .execute(r#"to_json(#{ id: 1, name: "Alice" })"#, &empty_ctx())
            .unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&result.value).unwrap();
        assert_eq!(parsed["id"], 1);
        assert_eq!(parsed["name"], "Alice");
    }

    #[test]
    fn parse_json_then_to_json_roundtrip_array_of_objects() {
        let engine = ScriptEngine::new();
        let ctx = ScriptContext {
            body: r#"[{"sku":"A1","qty":2},{"sku":"B2","qty":5}]"#.into(),
            ..empty_ctx()
        };
        let script = r#"
            let items = parse_json(request.body);
            let out = [];
            for it in items {
                out.push(#{ sku: it.sku, doubled: it.qty * 2 });
            }
            to_json(out)
        "#;
        let result = engine.execute(script, &ctx).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&result.value).unwrap();
        assert_eq!(parsed.as_array().unwrap().len(), 2);
        assert_eq!(parsed[0]["sku"], "A1");
        assert_eq!(parsed[0]["doubled"], 4);
        assert_eq!(parsed[1]["sku"], "B2");
        assert_eq!(parsed[1]["doubled"], 10);
    }

    #[test]
    fn parse_json_then_to_json_empty_array_stays_empty() {
        let engine = ScriptEngine::new();
        let ctx = ScriptContext {
            body: "[]".into(),
            ..empty_ctx()
        };
        let script = r#"
            let items = parse_json(request.body);
            let out = [];
            for it in items { out.push(it); }
            to_json(out)
        "#;
        let result = engine.execute(script, &ctx).unwrap();
        assert_eq!(result.value, "[]");
    }

    // --- parse_xml_items / xml_element ---

    #[test]
    fn parse_xml_items_extracts_repeated_flat_elements() {
        let engine = ScriptEngine::new();
        let ctx = ScriptContext {
            body: "<products><product><sku>A1</sku><qty>2</qty></product><product><sku>B2</sku><qty>5</qty></product></products>".into(),
            ..empty_ctx()
        };
        let result = engine
            .execute(
                r#"parse_xml_items(request.body, "products/product").len()"#,
                &ctx,
            )
            .unwrap();
        assert_eq!(result.value, "2");
        let result = engine
            .execute(
                r#"parse_xml_items(request.body, "products/product")[0].sku"#,
                &ctx,
            )
            .unwrap();
        assert_eq!(result.value, "A1");
        let result = engine
            .execute(
                r#"parse_xml_items(request.body, "products/product")[1].qty"#,
                &ctx,
            )
            .unwrap();
        assert_eq!(result.value, "5");
    }

    #[test]
    fn parse_xml_items_ignores_namespace_prefixes() {
        let engine = ScriptEngine::new();
        let ctx = ScriptContext {
            body: r#"<soap:Envelope><soap:Body><products><product><sku>A1</sku></product></products></soap:Body></soap:Envelope>"#.into(),
            ..empty_ctx()
        };
        let result = engine
            .execute(
                r#"parse_xml_items(request.body, "Envelope/Body/products/product").len()"#,
                &ctx,
            )
            .unwrap();
        assert_eq!(result.value, "1");
    }

    #[test]
    fn parse_xml_items_extracts_single_soap_operation_value_with_header_sibling() {
        // Extracting ONE value from a realistic SOAP body: an empty <Header></Header> written in full next to <Body>, and
        // several children (Nom, Siret) in the operation element. parse_xml_items() goes down to the operation element,
        // not to the leaf, then `[0].Field`: the operation appears once under Body, so the array always has one item.
        let engine = ScriptEngine::new();
        let ctx = ScriptContext {
            body: r#"<SOAP:Envelope><SOAP-ENV:Header></SOAP-ENV:Header><SOAP-ENV:Body><ns3:recherche><ns3:Nom>Test</ns3:Nom><ns3:Siret>12345678901234</ns3:Siret></ns3:recherche></SOAP-ENV:Body></SOAP:Envelope>"#.into(),
            ..empty_ctx()
        };
        let len = engine
            .execute(
                r#"parse_xml_items(request.body, "Envelope/Body/recherche").len()"#,
                &ctx,
            )
            .unwrap();
        assert_eq!(len.value, "1");
        let siret = engine
            .execute(
                r#"parse_xml_items(request.body, "Envelope/Body/recherche")[0].Siret"#,
                &ctx,
            )
            .unwrap();
        assert_eq!(siret.value, "12345678901234");
        let nom = engine
            .execute(
                r#"parse_xml_items(request.body, "Envelope/Body/recherche")[0].Nom"#,
                &ctx,
            )
            .unwrap();
        assert_eq!(nom.value, "Test");
    }

    #[test]
    fn parse_xml_items_no_match_returns_empty_array() {
        let engine = ScriptEngine::new();
        let ctx = ScriptContext {
            body: "<products></products>".into(),
            ..empty_ctx()
        };
        let result = engine
            .execute(
                r#"parse_xml_items(request.body, "products/product").len()"#,
                &ctx,
            )
            .unwrap();
        assert_eq!(result.value, "0");
    }

    /// The items `parse_xml_items` finds, each as its (field, text) pairs in tag order.
    fn xml_items(xml: &str, path: &str) -> Vec<Vec<(String, String)>> {
        parse_xml_items_impl(xml, path)
            .unwrap()
            .into_iter()
            .map(|item| {
                item.cast::<rhai::Map>()
                    .into_iter()
                    .map(|(key, value)| {
                        (
                            key.to_string(),
                            value.cast::<rhai::ImmutableString>().to_string(),
                        )
                    })
                    .collect()
            })
            .collect()
    }

    fn fields(pairs: &[(&str, &str)]) -> Vec<(String, String)> {
        pairs
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect()
    }

    #[test]
    fn parse_xml_items_reads_items_after_a_sibling_that_is_no_item() {
        let xml = "<products><count>2</count><product><sku>A</sku></product><product><sku>B</sku></product></products>";
        assert_eq!(
            xml_items(xml, "products/product"),
            vec![fields(&[("sku", "A")]), fields(&[("sku", "B")])]
        );
        // A path down to the leaf: each Siret is an item, whatever sibling comes before it.
        let xml = "<r><Nom>Test</Nom><Siret>123</Siret></r>";
        assert_eq!(xml_items(xml, "r/Siret").len(), 1);
    }

    #[test]
    fn parse_xml_items_ignores_a_parent_that_closed_before_the_rest_of_the_path() {
        let xml = "<d><root></root><list><item><v>1</v></item></list></d>";
        assert_eq!(
            xml_items(xml, "d/root/list/item"),
            Vec::<Vec<(String, String)>>::new()
        );
    }

    #[test]
    fn parse_xml_items_reads_a_path_from_the_root_element() {
        let xml = "<e><b><list><item><v>1</v></item></list></b></e>";
        assert_eq!(xml_items(xml, "e/b/list/item"), vec![fields(&[("v", "1")])]);
        for below_the_root_or_skipping_a_level in ["item", "list/item", "e/list/item"] {
            assert!(xml_items(xml, below_the_root_or_skipping_a_level).is_empty());
        }
    }

    #[test]
    fn parse_xml_items_ignores_a_parent_name_found_in_a_sibling_branch() {
        let xml = "<root><other><list><item><v>X</v></item></list></other><list><item><v>Y</v></item></list></root>";
        assert_eq!(
            xml_items(xml, "root/list/item"),
            vec![fields(&[("v", "Y")])]
        );
    }

    #[test]
    fn parse_xml_items_reads_a_self_closing_item_or_field_as_an_empty_one() {
        let xml = "<r><i><a/><b>2</b></i><i/><i><a></a></i></r>";
        assert_eq!(
            xml_items(xml, "r/i"),
            vec![
                fields(&[("a", ""), ("b", "2")]),
                vec![],
                fields(&[("a", "")])
            ]
        );
        // A self-closing parent holds no item, and closes at once.
        assert_eq!(
            xml_items("<r><p/><i><a>1</a></i></r>", "r/p/i"),
            Vec::<Vec<(String, String)>>::new()
        );
    }

    #[test]
    fn parse_xml_items_stops_at_the_first_malformed_part_keeping_the_items_before_it() {
        let xml = "<r><i><v>1</v></i></x><i><v>2</v></i></r>";
        assert_eq!(xml_items(xml, "r/i"), vec![fields(&[("v", "1")])]);
    }

    fn xml_error(xml: &str, path: &str) -> String {
        parse_xml_items_impl(xml, path).unwrap_err().to_string()
    }

    // The parents outside an item count with the item and its open children: the field of an item under
    // `XML_DEPTH_LIMIT - 2` parents is the deepest element read, one parent more refuses the text, and so does one
    // element more than the limit beside the path or inside a field.
    #[test]
    fn parse_xml_items_reads_up_to_the_depth_limit_and_refuses_a_text_past_it() {
        use crate::engine::matcher::XML_DEPTH_LIMIT;
        use crate::engine::script_budget::XML_TOO_DEEP;
        let document = |parents: usize| {
            format!(
                "{}<i><f>x</f></i>{}",
                "<a>".repeat(parents),
                "</a>".repeat(parents)
            )
        };
        let path = |parents: usize| format!("{}i", "a/".repeat(parents));
        let deepest = XML_DEPTH_LIMIT - 2;
        assert_eq!(
            xml_items(&document(deepest), &path(deepest)),
            vec![vec![("f".to_string(), "x".to_string())]]
        );
        assert!(xml_error(&document(deepest + 1), &path(deepest + 1)).contains(XML_TOO_DEEP));
        let beside = format!("<r>{}</r>", "<z>".repeat(XML_DEPTH_LIMIT));
        assert!(xml_error(&beside, "r/i").contains(XML_TOO_DEEP));
        let in_a_field = format!("<r><i><f>{}</f></i></r>", "<z>".repeat(XML_DEPTH_LIMIT - 2));
        assert!(xml_error(&in_a_field, "r/i").contains(XML_TOO_DEEP));
        // A self-closing item or field is an element too.
        let self_closing = |parents: usize| format!("{}<i/>", "<a>".repeat(parents));
        assert_eq!(
            xml_items(
                &self_closing(XML_DEPTH_LIMIT - 1),
                &path(XML_DEPTH_LIMIT - 1)
            ),
            vec![vec![]]
        );
        assert!(
            xml_error(&self_closing(XML_DEPTH_LIMIT), &path(XML_DEPTH_LIMIT))
                .contains(XML_TOO_DEEP)
        );
        // Inside an item, the item counts too: a self-closing field under `XML_DEPTH_LIMIT - 1` parents is one level
        // too deep.
        let field = |parents: usize| format!("{}<i><f/></i>", "<a>".repeat(parents));
        assert_eq!(
            xml_items(&field(XML_DEPTH_LIMIT - 2), &path(XML_DEPTH_LIMIT - 2)),
            vec![vec![("f".to_string(), String::new())]]
        );
        assert!(
            xml_error(&field(XML_DEPTH_LIMIT - 1), &path(XML_DEPTH_LIMIT - 1))
                .contains(XML_TOO_DEEP)
        );
    }

    #[test]
    fn parse_xml_items_counts_the_fields_of_all_its_items_against_the_size_of_a_map() {
        use crate::engine::script_budget::{MAP_TOO_LARGE, MAX_MAP_SIZE};
        let document = |fields: usize| {
            let items: String = (0..fields / 2).map(|_| "<i><a>1</a><b/></i>").collect();
            format!(
                "<r>{items}{}</r>",
                if fields % 2 == 1 {
                    "<i><a>1</a></i>"
                } else {
                    ""
                }
            )
        };
        assert_eq!(
            parse_xml_items_impl(&document(MAX_MAP_SIZE), "r/i")
                .unwrap()
                .len(),
            MAX_MAP_SIZE / 2
        );
        assert!(xml_error(&document(MAX_MAP_SIZE + 1), "r/i").contains(MAP_TOO_LARGE));
    }

    // Field names count with the texts, the texts of the fields already read once trimmed; a text read in several
    // pieces (`ab`, then the reference `&amp;`, then the rest) counts each piece as it comes.
    #[test]
    fn parse_xml_items_counts_field_names_and_texts_against_the_size_of_a_text() {
        use crate::engine::script_budget::{MAX_STRING_SIZE, TEXT_TOO_LARGE};
        let document = |total: usize| {
            let rest = "z".repeat(total - "a".len() - "xx".len() - "bb".len() - "ab&".len());
            format!("<r><i><a> xx </a><bb>ab&amp;{rest}</bb></i></r>")
        };
        let items = xml_items(&document(MAX_STRING_SIZE), "r/i");
        assert_eq!(items[0][0], ("a".to_string(), "xx".to_string()));
        assert_eq!(items[0][1].1.len(), MAX_STRING_SIZE - 5);
        assert!(xml_error(&document(MAX_STRING_SIZE + 1), "r/i").contains(TEXT_TOO_LARGE));
    }

    // The name of a field without text counts too, written self-closing or in full.
    #[test]
    fn parse_xml_items_counts_the_name_of_a_field_without_text_against_the_size_of_a_text() {
        use crate::engine::script_budget::{MAX_STRING_SIZE, TEXT_TOO_LARGE};
        for self_closing in [true, false] {
            let document = |length: usize| {
                let name = "x".repeat(length);
                let field = if self_closing {
                    format!("<{name}/>")
                } else {
                    format!("<{name}></{name}>")
                };
                format!("<r><i>{field}</i></r>")
            };
            let items = xml_items(&document(MAX_STRING_SIZE), "r/i");
            assert_eq!(items[0][0].0.len(), MAX_STRING_SIZE);
            assert!(xml_error(&document(MAX_STRING_SIZE + 1), "r/i").contains(TEXT_TOO_LARGE));
        }
    }

    /// A node of a generated document. An element has a local name, maybe a namespace prefix and an attribute, and
    /// its children, written self-closing or not when it has none.
    #[derive(Clone, Debug)]
    enum XmlNode {
        Element {
            name: &'static str,
            prefix: Option<&'static str>,
            attribute: bool,
            self_closing: bool,
            children: Vec<XmlNode>,
        },
        Text(String),
        Comment,
    }

    fn write_xml_node(node: &XmlNode, out: &mut String) {
        match node {
            // `é` as a character reference, any other character beyond ASCII as it is.
            XmlNode::Text(text) => text.chars().for_each(|c| match c {
                '&' => out.push_str("&amp;"),
                '<' => out.push_str("&lt;"),
                '>' => out.push_str("&gt;"),
                'é' => out.push_str("&#233;"),
                _ => out.push(c),
            }),
            XmlNode::Comment => out.push_str("<!-- <i>c</i> -->"),
            XmlNode::Element {
                name,
                prefix,
                attribute,
                self_closing,
                children,
            } => {
                let tag = prefix.map_or(name.to_string(), |p| format!("{p}:{name}"));
                let attribute = if *attribute {
                    r#" i="b&amp;&quot;""#
                } else {
                    ""
                };
                if children.is_empty() && *self_closing {
                    out.push_str(&format!("<{tag}{attribute}/>"));
                } else {
                    out.push_str(&format!("<{tag}{attribute}>"));
                    children.iter().for_each(|child| write_xml_node(child, out));
                    out.push_str(&format!("</{tag}>"));
                }
            }
        }
    }

    /// The reference walk: the elements named as the last segment whose chain of ancestors, from the top of the
    /// document, is exactly the rest of the path; each one gives its child elements, valued by their own texts.
    fn reference_items(
        nodes: &[XmlNode],
        ancestors: &mut Vec<&'static str>,
        path: &[&str],
        found: &mut Vec<Vec<(String, String)>>,
    ) {
        for node in nodes {
            let XmlNode::Element { name, children, .. } = node else {
                continue;
            };
            if ancestors.len() + 1 == path.len() && ancestors.iter().chain([name]).eq(path.iter()) {
                let mut item = std::collections::BTreeMap::new();
                for child in children {
                    if let XmlNode::Element {
                        name,
                        children: texts,
                        ..
                    } = child
                    {
                        let text: String = texts
                            .iter()
                            .filter_map(|t| match t {
                                XmlNode::Text(text) => Some(text.as_str()),
                                _ => None,
                            })
                            .collect();
                        item.insert(name.to_string(), text.trim().to_string());
                    }
                }
                found.push(item.into_iter().collect());
            } else {
                ancestors.push(name);
                reference_items(children, ancestors, path, found);
                ancestors.pop();
            }
        }
    }

    /// The chain of names from the top of the document down to each element.
    fn element_chains(
        nodes: &[XmlNode],
        ancestors: &mut Vec<&'static str>,
        chains: &mut Vec<Vec<&'static str>>,
    ) {
        for node in nodes {
            if let XmlNode::Element { name, children, .. } = node {
                ancestors.push(name);
                chains.push(ancestors.clone());
                element_chains(children, ancestors, chains);
                ancestors.pop();
            }
        }
    }

    /// Documents of a few elements whose names repeat at every level (a parent's name in a sibling branch, deeper,
    /// or closed before the rest of the path), with prefixes, attributes, comments, self-closing elements, and texts
    /// that need escaping, hold references, line breaks and characters beyond ASCII; with each, a path of one to four
    /// segments over the same names, half of them the chain of an element of the document. Not generated: CDATA,
    /// processing instructions, a DTD, malformed documents, the limits of a value, prefixes in the path, depths near
    /// `XML_DEPTH_LIMIT`.
    fn generated_documents()
    -> impl proptest::strategy::Strategy<Value = (Vec<XmlNode>, Vec<&'static str>)> {
        use proptest::prelude::*;
        let name = prop::sample::select(vec!["a", "b", "i"]);
        let text = "[a-c &<>\"' \n\téü]{0,4}".prop_map(XmlNode::Text);
        let leaf = prop_oneof![
            4 => text.clone(),
            1 => Just(XmlNode::Comment),
            4 => (name.clone(), any::<bool>(), any::<bool>()).prop_map(|(name, attribute, self_closing)| {
                XmlNode::Element {
                    name,
                    prefix: None,
                    attribute,
                    self_closing,
                    children: vec![],
                }
            }),
        ];
        let node = leaf.prop_recursive(5, 48, 4, move |inner| {
            (
                name.clone(),
                prop::option::of(prop::sample::select(vec!["n", "soap"])),
                any::<bool>(),
                any::<bool>(),
                prop::collection::vec(inner, 0..4),
            )
                .prop_map(|(name, prefix, attribute, self_closing, children)| {
                    XmlNode::Element {
                        name,
                        prefix,
                        attribute,
                        self_closing,
                        children,
                    }
                })
        });
        (
            prop::collection::vec(node, 1..3),
            prop::collection::vec(prop::sample::select(vec!["a", "b", "i"]), 1..5),
            prop::option::of(any::<prop::sample::Index>()),
        )
            .prop_map(|(nodes, drawn, in_tree)| {
                let mut chains = Vec::new();
                element_chains(&nodes, &mut Vec::new(), &mut chains);
                let path = match in_tree {
                    Some(index) if !chains.is_empty() => chains[index.index(chains.len())].clone(),
                    _ => drawn,
                };
                (nodes, path)
            })
    }

    fn written(nodes: &[XmlNode]) -> String {
        let mut xml = String::new();
        nodes.iter().for_each(|node| write_xml_node(node, &mut xml));
        xml
    }

    // A quarter of the cases at least must expect items, and some of them a text: a judge whose cases all expect
    // nothing would pass on any code.
    #[test]
    fn parse_xml_items_finds_what_a_walk_of_the_tree_finds() {
        use proptest::prelude::*;
        use proptest::test_runner::{Config, RngSeed, TestRunner};
        let config = Config {
            cases: 1024,
            rng_seed: RngSeed::Fixed(0x786d_6c5f_6974_656d),
            failure_persistence: None,
            ..Config::default()
        };
        let (with_items, with_text) = (std::cell::Cell::new(0), std::cell::Cell::new(0));
        let result = TestRunner::new(config).run(&generated_documents(), |(nodes, path)| {
            let xml = written(&nodes);
            let mut expected = Vec::new();
            reference_items(&nodes, &mut Vec::new(), &path, &mut expected);
            with_items.set(with_items.get() + usize::from(!expected.is_empty()));
            let has_text = expected.iter().flatten().any(|(_, text)| !text.is_empty());
            with_text.set(with_text.get() + usize::from(has_text));
            prop_assert_eq!(xml_items(&xml, &path.join("/")), expected, "{}", xml);
            Ok(())
        });
        if let Err(failure) = result {
            panic!("{failure}");
        }
        assert!(
            with_items.get() >= 256 && with_text.get() >= 64,
            "{with_items:?} {with_text:?}"
        );
    }

    /// The reference reading of an XPath condition: the first element, in the order of the document, whose chain of
    /// names from the top is the path. Its value is the text from its start tag to the first end tag after it: its
    /// own, or the end tag of the first child written with one, read the same way.
    fn reference_xpath(
        nodes: &[XmlNode],
        ancestors: &mut Vec<&'static str>,
        path: &[&str],
    ) -> Option<String> {
        for node in nodes {
            let XmlNode::Element { name, children, .. } = node else {
                continue;
            };
            ancestors.push(name);
            let found = if ancestors.iter().eq(path.iter()) {
                let mut text = String::new();
                text_to_the_first_end_tag(children, &mut text);
                Some(text)
            } else {
                reference_xpath(children, ancestors, path)
            };
            ancestors.pop();
            if found.is_some() {
                return found;
            }
        }
        None
    }

    fn text_to_the_first_end_tag(nodes: &[XmlNode], out: &mut String) {
        for node in nodes {
            match node {
                XmlNode::Text(text) => out.push_str(text),
                XmlNode::Element {
                    children,
                    self_closing,
                    ..
                } if !(children.is_empty() && *self_closing) => {
                    text_to_the_first_end_tag(children, out);
                    return;
                }
                _ => {}
            }
        }
    }

    // The reader of XPath conditions and `{{xpath.…}}` variables (`MatchEngine::extract_xpath`), judged on the
    // documents above. A quarter of the cases at least must find a value, and some of them a text.
    #[test]
    fn extract_xpath_finds_what_a_walk_of_the_tree_finds() {
        use proptest::prelude::*;
        use proptest::test_runner::{Config, RngSeed, TestRunner};
        let config = Config {
            cases: 1024,
            rng_seed: RngSeed::Fixed(0x7870_6174_685f_6a64),
            failure_persistence: None,
            ..Config::default()
        };
        let (with_value, with_text) = (std::cell::Cell::new(0), std::cell::Cell::new(0));
        let result = TestRunner::new(config).run(&generated_documents(), |(nodes, path)| {
            let xml = written(&nodes);
            let expected = reference_xpath(&nodes, &mut Vec::new(), &path);
            with_value.set(with_value.get() + usize::from(expected.is_some()));
            let has_text = expected.as_deref().is_some_and(|text| !text.is_empty());
            with_text.set(with_text.get() + usize::from(has_text));
            prop_assert_eq!(
                crate::engine::matcher::MatchEngine::extract_xpath(xml.as_bytes(), &path.join("/")),
                expected,
                "{}",
                xml
            );
            Ok(())
        });
        if let Err(failure) = result {
            panic!("{failure}");
        }
        assert!(
            with_value.get() >= 256 && with_text.get() >= 64,
            "{with_value:?} {with_text:?}"
        );
    }

    #[test]
    fn xml_element_scalar_escapes_special_chars() {
        let engine = ScriptEngine::new();
        let result = engine
            .execute(r#"xml_element("name", "A & <B>")"#, &empty_ctx())
            .unwrap();
        assert_eq!(result.value, "<name>A &amp; &lt;B&gt;</name>");
    }

    // Any text comes back whole when the element is read, and none of the five characters of XML markup is left as
    // it is between the tags.
    #[test]
    fn xml_element_escapes_every_markup_character_of_a_text() {
        use proptest::prelude::*;
        use proptest::test_runner::{Config, RngSeed, TestRunner};
        let config = Config {
            cases: 256,
            rng_seed: RngSeed::Fixed(0x6573_6361_7065_5f74),
            failure_persistence: None,
            ..Config::default()
        };
        let result = TestRunner::new(config).run(&"[a-c&<>\"' é]{0,12}", |text| {
            let xml = xml_element_impl("t", &rhai::Dynamic::from(text.clone())).unwrap();
            let inner = &xml["<t>".len()..xml.len() - "</t>".len()];
            prop_assert!(!inner.contains(['<', '>', '"', '\'']), "{}", xml);
            prop_assert_eq!(
                crate::engine::matcher::MatchEngine::extract_xpath(xml.as_bytes(), "t"),
                Some(text.clone())
            );
            Ok(())
        });
        if let Err(failure) = result {
            panic!("{failure}");
        }
    }

    #[test]
    fn xml_element_builds_a_text_up_to_the_size_of_a_text_and_not_one_byte_more() {
        use crate::engine::script_budget::{MAX_STRING_SIZE, TEXT_TOO_LARGE};
        let element = |length: usize| {
            xml_element_impl(
                "t",
                &rhai::Dynamic::from("x".repeat(length - "<t></t>".len())),
            )
        };
        assert_eq!(element(MAX_STRING_SIZE).unwrap().len(), MAX_STRING_SIZE);
        assert!(
            element(MAX_STRING_SIZE + 1)
                .unwrap_err()
                .to_string()
                .contains(TEXT_TOO_LARGE)
        );
    }

    #[test]
    fn xml_element_map_builds_nested_children() {
        let engine = ScriptEngine::new();
        let result = engine
            .execute(
                r#"xml_element("product", #{ sku: "A1", qty: 2 })"#,
                &empty_ctx(),
            )
            .unwrap();
        assert!(result.value.starts_with("<product>"));
        assert!(result.value.ends_with("</product>"));
        assert!(result.value.contains("<sku>A1</sku>"));
        assert!(result.value.contains("<qty>2</qty>"));
    }

    #[test]
    fn parse_xml_items_then_xml_element_roundtrip_builds_same_count() {
        let engine = ScriptEngine::new();
        let ctx = ScriptContext {
            body: "<products><product><sku>A1</sku><qty>2</qty></product><product><sku>B2</sku><qty>5</qty></product><product><sku>C3</sku><qty>9</qty></product></products>".into(),
            ..empty_ctx()
        };
        let script = r#"
            let items = parse_xml_items(request.body, "products/product");
            let out = "";
            for it in items {
                out += xml_element("item", #{ sku: it.sku, doubledQty: parse_int(it.qty) * 2 });
            }
            out
        "#;
        let result = engine.execute(script, &ctx).unwrap();
        assert_eq!(result.value.matches("<item>").count(), 3);
        assert!(result.value.contains("<sku>A1</sku>"));
        assert!(result.value.contains("<doubledQty>4</doubledQty>"));
        assert!(result.value.contains("<sku>C3</sku>"));
        assert!(result.value.contains("<doubledQty>18</doubledQty>"));
    }

    // --- Representative scripts (see docs/en/rhai-scripts.md): lookup tables, loops with conditions, every request
    // source in one script. Map indexing, `.contains()`, `in`, `.get()` and `switch` all work, and a missing key gives
    // an empty value rather than an error.

    #[test]
    fn map_lookup_returns_mapped_value_for_known_key() {
        let engine = ScriptEngine::new();
        let script = r#"
            let mapping = #{ "svc-a": "id-1", "svc-b": "id-2" };
            let key = request.path.name;
            if mapping.contains(key) { mapping[key] } else { "unknown" }
        "#;
        let mut path = HashMap::new();
        path.insert("name".into(), "svc-b".into());
        let ctx = ScriptContext {
            path_params: path,
            ..empty_ctx()
        };
        let result = engine.execute(script, &ctx).unwrap();
        assert_eq!(result.value, "id-2");
    }

    #[test]
    fn map_lookup_falls_back_for_unknown_key() {
        let engine = ScriptEngine::new();
        let script = r#"
            let mapping = #{ "svc-a": "id-1", "svc-b": "id-2" };
            let key = request.path.name;
            if mapping.contains(key) { mapping[key] } else { "unknown" }
        "#;
        let mut path = HashMap::new();
        path.insert("name".into(), "svc-zzz".into());
        let ctx = ScriptContext {
            path_params: path,
            ..empty_ctx()
        };
        let result = engine.execute(script, &ctx).unwrap();
        assert_eq!(result.value, "unknown");
    }

    #[test]
    fn map_lookup_via_switch_expression() {
        // `switch`, the other documented form of the lookup above.
        let engine = ScriptEngine::new();
        let script = r#"
            let key = request.path.name;
            switch key {
                "svc-a" => "id-1",
                "svc-b" => "id-2",
                _ => "unknown"
            }
        "#;
        let mut path = HashMap::new();
        path.insert("name".into(), "svc-a".into());
        let ctx = ScriptContext {
            path_params: path,
            ..empty_ctx()
        };
        let result = engine.execute(script, &ctx).unwrap();
        assert_eq!(result.value, "id-1");
    }

    #[test]
    fn loop_with_condition_counts_matching_items() {
        // A loop with a condition: counts the JSON lines above a quantity threshold and returns a summary map.
        let engine = ScriptEngine::new();
        let ctx = ScriptContext {
            body: r#"[{"sku":"A","qty":1},{"sku":"B","qty":5},{"sku":"C","qty":12}]"#.into(),
            ..empty_ctx()
        };
        let script = r#"
            let items = parse_json(request.body);
            let big_count = 0;
            let skus = [];
            for it in items {
                if it.qty > 3 {
                    big_count += 1;
                    skus.push(it.sku);
                }
            }
            #{ big_count: big_count, big_skus: to_json(skus) }
        "#;
        let result = engine.execute(script, &ctx).unwrap();
        assert_eq!(result.fields.get("big_count").unwrap(), "2");
        let skus: serde_json::Value =
            serde_json::from_str(result.fields.get("big_skus").unwrap()).unwrap();
        assert_eq!(skus, serde_json::json!(["B", "C"]));
    }

    #[test]
    fn combines_path_query_headers_and_body_in_one_script() {
        // Every request source in one script (path, query, headers, body), with an if/else fallback for a missing header.
        // Rhai has no ternary operator (`?:` is "Unknown operator: '?'"): if/else is the only form.
        let engine = ScriptEngine::new();
        let mut path = HashMap::new();
        path.insert("id".into(), "42".into());
        let mut query = HashMap::new();
        query.insert("verbose".into(), "true".into());
        let mut headers = HashMap::new();
        headers.insert("x-request-id".into(), "req-abc".into());
        let ctx = ScriptContext {
            body: r#"{"note":"hello"}"#.into(),
            headers,
            query_params: query,
            path_params: path,
        };
        let script = r#"
            let id = request.path.id;
            let verbose = request.query.verbose;
            let req_id = if request.headers.contains("x-request-id") { request.headers["x-request-id"] } else { "none" };
            let missing = if request.headers.contains("x-absent") { request.headers["x-absent"] } else { "none" };
            let note = parse_json(request.body).note;
            #{ id: id, verbose: verbose, req_id: req_id, missing: missing, note: note }
        "#;
        let result = engine.execute(script, &ctx).unwrap();
        assert_eq!(result.fields.get("id").unwrap(), "42");
        assert_eq!(result.fields.get("verbose").unwrap(), "true");
        assert_eq!(result.fields.get("req_id").unwrap(), "req-abc");
        assert_eq!(result.fields.get("missing").unwrap(), "none");
        assert_eq!(result.fields.get("note").unwrap(), "hello");
    }

    // Calling a function that does not exist IS a runtime error, unlike reading a missing map key, which silently gives
    // an empty value (tests above). execute() returns it as Err; run_rule_script (intercept.rs) swallows it, and the
    // rule tester (/api/rule-test) shows it.

    #[test]
    fn calling_undefined_function_is_a_real_execution_error() {
        let engine = ScriptEngine::new();
        let result = engine.execute("totally_undefined_fn(1, 2)", &empty_ctx());
        let err =
            result.expect_err("calling a function that does not exist must be a runtime error");
        assert!(err.contains("totally_undefined_fn"));
    }

    // --- An object picked from a list, then its fields reused. The script never fails in the cases below:
    // (a) returning the picked object itself gives every scalar field, each available as {{script.field}};
    // (b) once the picked object is combined with something else (`#{ city: pick, id: uuid() }`), "city" is a nested
    //     map, rendered as valid JSON by dynamic_field_to_string rather than with Rhai's display form;
    // (c) a missing field (a typo, or a nested path such as {{script.city.name}}: only one level exists) is no error
    //     either, for maps built by the script as for `request.*`.

    #[test]
    fn seeded_pick_on_object_list_returned_directly_exposes_all_scalar_fields() {
        // Case (a): the picked object is the script's last expression.
        let engine = ScriptEngine::new();
        let script = r#"
            let villes = [
                #{ name: "Paris", cp: "75000", insee: "75056" },
                #{ name: "Lyon", cp: "69000", insee: "69123" }
            ];
            seeded_pick(request.path.siret, villes)
        "#;
        let mut path = HashMap::new();
        path.insert("siret".into(), "44306184100047".into());
        let ctx = ScriptContext {
            path_params: path,
            ..empty_ctx()
        };
        let result = engine.execute(script, &ctx).unwrap();
        assert_eq!(result.fields.get("name").unwrap(), "Lyon");
        assert_eq!(result.fields.get("cp").unwrap(), "69000");
        assert_eq!(result.fields.get("insee").unwrap(), "69123");
    }

    #[test]
    fn seeded_pick_wrapped_in_a_map_field_serializes_to_valid_json_not_rhai_debug_syntax() {
        // Case (b), the defect found: an object nested under a key of the returned map must render as valid JSON, usable in
        // an advanced template, never as Rhai's display form (`#{"k": "v", ...}`).
        let engine = ScriptEngine::new();
        let script = r#"
            let villes = [
                #{ name: "Paris", cp: "75000", insee: "75056" },
                #{ name: "Lyon", cp: "69000", insee: "69123" }
            ];
            let ville = seeded_pick(request.path.siret, villes);
            #{ ville: ville, other: "x" }
        "#;
        let mut path = HashMap::new();
        path.insert("siret".into(), "44306184100047".into());
        let ctx = ScriptContext {
            path_params: path,
            ..empty_ctx()
        };
        let result = engine.execute(script, &ctx).unwrap();
        assert_eq!(result.fields.get("other").unwrap(), "x");
        let raw = result.fields.get("ville").unwrap();
        assert!(
            !raw.starts_with('#'),
            "the nested field must not use the Rhai #{{...}} syntax: {raw}"
        );
        let parsed: serde_json::Value = serde_json::from_str(raw)
            .unwrap_or_else(|e| panic!("the nested field must be valid JSON, got {raw:?}: {e}"));
        assert_eq!(parsed["name"], "Lyon");
        assert_eq!(parsed["cp"], "69000");
        assert_eq!(parsed["insee"], "69123");
    }

    #[test]
    fn seeded_pick_wrapped_array_of_objects_also_serializes_to_valid_json() {
        // The same for an array of objects under a key.
        let engine = ScriptEngine::new();
        let script = r#"
            #{ items: [#{ sku: "A1", qty: 2 }, #{ sku: "B2", qty: 5 }] }
        "#;
        let result = engine.execute(script, &empty_ctx()).unwrap();
        let raw = result.fields.get("items").unwrap();
        let parsed: serde_json::Value = serde_json::from_str(raw).unwrap();
        assert_eq!(parsed.as_array().unwrap().len(), 2);
        assert_eq!(parsed[0]["sku"], "A1");
        assert_eq!(parsed[1]["qty"], 5);
    }

    #[test]
    fn scalar_map_field_stringification_is_unchanged_by_the_json_fix() {
        // A scalar field (string, integer, boolean) still renders as plain text, without JSON quotes: only nested maps and
        // arrays changed, or {{script.field}} would break for nearly every existing script.
        let engine = ScriptEngine::new();
        let result = engine
            .execute(
                r#"#{ greeting: "hi", count: 42, active: true }"#,
                &empty_ctx(),
            )
            .unwrap();
        assert_eq!(result.fields.get("greeting").unwrap(), "hi");
        assert_eq!(result.fields.get("count").unwrap(), "42");
        assert_eq!(result.fields.get("active").unwrap(), "true");
    }

    #[test]
    fn accessing_a_typo_field_or_unsupported_nested_path_never_errors_but_stays_empty() {
        // Case (c): neither a typo (`city.nom` for `city.name`) nor a nested template path raises an error, for a map built
        // by the script just as for `request.*`.
        let engine = ScriptEngine::new();
        let script = r#"
            let villes = [ #{ name: "Paris", cp: "75000", insee: "75056" } ];
            let ville = seeded_pick(request.path.siret, villes);
            ville.nom
        "#;
        let mut path = HashMap::new();
        path.insert("siret".into(), "44306184100047".into());
        let ctx = ScriptContext {
            path_params: path,
            ..empty_ctx()
        };
        let result = engine.execute(script, &ctx);
        assert!(
            result.is_ok(),
            "a missing field of a map built by the script must never be an error"
        );
        assert_eq!(result.unwrap().value, "");
    }

    #[test]
    fn missing_map_key_access_is_not_an_error_unlike_undefined_function() {
        // Unlike a missing function (test above), a missing map key is not an error. That is why a dry run with a made-up
        // request could not catch a wrong key name, and why the rule tester replays a real captured request.
        let engine = ScriptEngine::new();
        let result = engine.execute("request.path.this_key_does_not_exist", &empty_ctx());
        assert!(result.is_ok(), "a missing key must never be an error");
        assert_eq!(result.unwrap().value, "");
    }

    #[test]
    fn import_cannot_load_a_script_file_from_the_disk() {
        let dir = crate::server::test_support::temp_data_dir("rhai-import");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("secret.rhai"), "fn read() { \"leaked\" }").unwrap();
        let module = dir.join("secret").to_string_lossy().replace('\\', "/");
        let script = format!("import \"{module}\" as m; m::read()");
        let result = ScriptEngine::new().execute(&script, &empty_ctx());
        let _ = std::fs::remove_dir_all(&dir);
        assert!(result.is_err(), "a rule script read a file: {result:?}");
    }

    #[test]
    fn eval_is_not_available_to_rule_scripts() {
        assert!(
            ScriptEngine::new()
                .execute("eval(\"40 + 2\")", &empty_ctx())
                .is_err()
        );
        assert!(ScriptEngine::new().validate("eval(\"40 + 2\")").is_err());
    }

    #[test]
    fn print_and_debug_are_accepted_without_writing_to_stdout() {
        let result = ScriptEngine::new().execute("print(\"x\"); debug(\"y\"); 1", &empty_ctx());
        assert_eq!(result.unwrap().value, "1");
    }

    #[test]
    fn random_and_seeded_ints_accept_the_full_i64_range() {
        let engine = ScriptEngine::new();
        let full = "-9223372036854775807 - 1, 9223372036854775807";
        for script in [
            format!("random_int({full})"),
            format!("seeded_int(\"seed\", {full})"),
        ] {
            let result = engine.execute(&script, &empty_ctx());
            assert!(result.is_ok(), "{script}: {result:?}");
        }
        let value: i64 = engine
            .execute("seeded_int(\"seed\", -5, 5)", &empty_ctx())
            .unwrap()
            .value
            .parse()
            .unwrap();
        assert!((-5..=5).contains(&value));
    }

    #[test]
    fn date_offsets_out_of_any_calendar_do_not_panic() {
        let engine = ScriptEngine::new();
        for script in [
            "date_past(9223372036854775807)",
            "date_future(9223372036854775807, \"fr\")",
        ] {
            assert!(engine.execute(script, &empty_ctx()).is_ok(), "{script}");
        }
    }

    #[test]
    fn parse_date_rejects_years_wider_than_four_digits() {
        let result = ScriptEngine::new().execute(
            "parse_date(\"999999999999999999-01-01\", \"yyyyyyyyyyyyyyyyyy-MM-dd\")",
            &empty_ctx(),
        );
        assert!(result.is_err());
    }
}
