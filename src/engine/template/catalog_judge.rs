// The interface's help on templates (`frontend/src/lib/template-catalog.json`: the variables it suggests, the pipes
// with an example each, and the mistakes it warns about) judged by the engine that renders them. Every pipe example
// must give its output here, every trap the output the warning announces (an unknown pipe leaves the value as it is,
// a number argument that is not a number gives what the warning says), and every suggested variable a value.
//
// What it does not see: a pipe or variable of the engine that the catalogue leaves out (the engine has no list to
// compare with), and `script.`, which needs a script result.
use super::*;
use serde_json::Value;

const CATALOG: &str = include_str!("../../../frontend/src/lib/template-catalog.json");

fn catalog() -> Value {
    serde_json::from_str(CATALOG).expect("the catalogue is JSON")
}

fn text<'a>(v: &'a Value, key: &str) -> &'a str {
    v[key].as_str().unwrap_or_else(|| panic!("{key} in {v}"))
}

fn render(template: &str, body: &str) -> String {
    let path: HashMap<String, String> = [("k".to_string(), "v".to_string())].into();
    let (query, headers) = (path.clone(), path.clone());
    let ctx = TemplateContext {
        path_params: &path,
        query_params: &query,
        headers: &headers,
        request_body: body.as_bytes(),
        seq_counter: 7,
        script_result: None,
    };
    render_template(template, &ctx)
}

fn literal(input: &str, pipe: &str) -> String {
    format!("{{{{\"{input}\" | {pipe}}}}}")
}

#[test]
fn every_pipe_example_gives_its_output() {
    for pipe in catalog()["pipes"].as_array().unwrap() {
        let template = literal(text(pipe, "input"), text(pipe, "insert"));
        assert_eq!(render(&template, ""), text(pipe, "output"), "{template}");
        assert!(
            text(pipe, "insert").starts_with(text(pipe, "name")),
            "{pipe}"
        );
    }
}

#[test]
fn every_trap_gives_what_its_warning_announces() {
    for trap in catalog()["traps"].as_array().unwrap() {
        let template = literal(text(trap, "input"), text(trap, "pipe"));
        assert_eq!(render(&template, ""), text(trap, "output"), "{template}");
    }
}

#[test]
fn every_suggested_variable_gives_a_value_and_an_unknown_one_none() {
    let catalog = catalog();
    for variable in catalog["variables"].as_array().unwrap() {
        let Some(probe) = variable["probe"].as_str() else {
            continue;
        };
        let body = variable["body"].as_str().unwrap_or("");
        let value = render(&format!("{{{{{probe}}}}}"), body);
        assert!(!value.is_empty(), "{probe}");
        assert!(probe.starts_with(text(variable, "insert")), "{variable}");
    }
    let unknown = text(&catalog, "unknownVariable");
    assert_eq!(render(&format!("{{{{{unknown}}}}}"), ""), "");
}

#[test]
fn every_fake_kind_gives_a_value() {
    for kind in catalog()["fake"].as_array().unwrap() {
        let kind = kind.as_str().unwrap();
        assert!(
            !render(&format!("{{{{fake.{kind}}}}}"), "").is_empty(),
            "{kind}"
        );
    }
}
