//! A view's filter: its tree of conditions, read from a view config in the editor's form or from
//! the engine's filter tree, and whether a record passes it.
//!
//! Conditions use the editor's operators (`is`, `isNot`, `contains`, `isAnyOf`, …) or the
//! engine's (`eq`, `neq`, `gt`, `in`, …). A condition on a field the table does not have, or with
//! an operator neither knows, is left out. An editor condition the editor would not apply is left
//! out too: one whose operator the editor does not offer for the field's type (a field whose type
//! changed after the filter was made), or one still being written (an operator other than
//! `isEmpty`/`isNotEmpty` with no value yet: nothing, blank text, an empty list). A group with
//! nothing left in it keeps every record.

use std::cmp::Ordering;
use std::collections::HashMap;

use kition_table_file::{Field, FilterRule, FilterTree, FilterTreeChild};
use serde_json::{Map, Value};

use crate::value::{display, number, parse_time, truthy};

/// The editor's operators.
pub const EDITOR_OPERATORS: [&str; 22] = [
    "is",
    "isNot",
    "isEmpty",
    "isNotEmpty",
    "contains",
    "doesNotContain",
    "isGreater",
    "isGreaterEqual",
    "isLess",
    "isLessEqual",
    "isAnyOf",
    "isNoneOf",
    "hasAnyOf",
    "hasAllOf",
    "isExactly",
    "isNotExactly",
    "hasNoneOf",
    "isWithIn",
    "isBefore",
    "isAfter",
    "isOnOrBefore",
    "isOnOrAfter",
];

/// The editor's operators the editor offers for a field of type `kind` (its filter menu): none
/// for the types it does not filter.
pub fn editor_operators(kind: &str) -> &'static [&'static str] {
    const TEXT: &[&str] = &["is", "isNot", "contains", "doesNotContain", "isEmpty", "isNotEmpty"];
    const NUMBER: &[&str] = &["is", "isNot", "isGreater", "isGreaterEqual", "isLess", "isLessEqual", "isEmpty", "isNotEmpty"];
    const DATE: &[&str] = &["is", "isNot", "isWithIn", "isBefore", "isAfter", "isOnOrBefore", "isOnOrAfter", "isEmpty", "isNotEmpty"];
    const SINGLE_SELECT: &[&str] = &["is", "isNot", "isAnyOf", "isNoneOf", "isEmpty", "isNotEmpty"];
    const MULTI_SELECT: &[&str] = &["hasAnyOf", "hasAllOf", "isExactly", "isNotExactly", "hasNoneOf", "isEmpty", "isNotEmpty"];
    const CHECKBOX: &[&str] = &["is"];
    const ATTACHMENT: &[&str] = &["isEmpty", "isNotEmpty"];
    match kind {
        "text" | "long_text" | "url" => TEXT,
        "number" | "rating" | "auto_number" => NUMBER,
        "date" | "datetime" | "created_time" | "last_modified_time" => DATE,
        "single_select" | "user" | "created_by" | "last_modified_by" => SINGLE_SELECT,
        "multi_select" => MULTI_SELECT,
        "checkbox" => CHECKBOX,
        "attachment" => ATTACHMENT,
        _ => &[],
    }
}

/// A filter tree in the editor's form (`{kind: "group", conjunction, children}` and
/// `{kind: "condition", field_name, operator, value}`) as the engine's, its fields by id
/// (`field_name` first, else `field_id`; 0 for neither). `None` when it holds no group.
pub fn tree_of_config(value: &Value, fields: &[Field]) -> Option<FilterTree> {
    if value.get("kind").and_then(Value::as_str) != Some("group") {
        return None;
    }
    let field_id = |item: &Value| {
        item.get("field_name")
            .and_then(Value::as_str)
            .and_then(|name| fields.iter().find(|field| field.name == name))
            .map(|field| field.id)
            .or_else(|| item.get("field_id").and_then(Value::as_i64))
            .unwrap_or(0)
    };
    let children = value
        .get("children")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|child| match child.get("kind").and_then(Value::as_str) {
            Some("group") => tree_of_config(child, fields).map(|group| FilterTreeChild { group: Some(Box::new(group)), condition: None }),
            Some("condition") => Some(FilterTreeChild {
                group: None,
                condition: Some(FilterRule {
                    field_id: field_id(child),
                    operator: child.get("operator").and_then(Value::as_str).unwrap_or_default().to_owned(),
                    value: child.get("value").cloned().unwrap_or(Value::Null),
                }),
            }),
            _ => None,
        })
        .collect();
    let conjunction = if value.get("conjunction").and_then(Value::as_str) == Some("or") { "or" } else { "and" };
    Some(FilterTree { conjunction: conjunction.to_owned(), children })
}

/// How many conditions a tree holds, in all its groups.
pub fn count_conditions(tree: &FilterTree) -> usize {
    tree.children
        .iter()
        .map(|child| match (&child.group, &child.condition) {
            (Some(group), _) => count_conditions(group),
            (None, Some(_)) => 1,
            _ => 0,
        })
        .sum()
}

/// A filter ready to test records: its conditions' fields found once.
#[derive(Clone, Debug)]
pub struct Filter {
    any: bool,
    nodes: Vec<Node>,
}

#[derive(Clone, Debug)]
enum Node {
    Group(Filter),
    Condition { name: String, kind: String, operator: String, value: Value },
}

impl Filter {
    /// `tree` over a table with `fields`; the conditions left out (see the module's notes) are
    /// dropped here.
    pub fn new(tree: &FilterTree, fields: &[Field]) -> Self {
        let by_id: HashMap<i64, &Field> = fields.iter().map(|field| (field.id, field)).collect();
        Self::compile(tree, &by_id)
    }

    fn compile(tree: &FilterTree, fields: &HashMap<i64, &Field>) -> Self {
        let nodes = tree
            .children
            .iter()
            .filter_map(|child| match (&child.group, &child.condition) {
                (Some(group), _) => Some(Node::Group(Self::compile(group, fields))),
                (None, Some(rule)) => {
                    let field = fields.get(&rule.field_id)?;
                    active(&field.field_type, &rule.operator, &rule.value).then(|| Node::Condition {
                        name: field.name.clone(),
                        kind: field.field_type.clone(),
                        operator: rule.operator.clone(),
                        value: rule.value.clone(),
                    })
                }
                _ => None,
            })
            .collect();
        Self { any: tree.conjunction.trim().eq_ignore_ascii_case("or"), nodes }
    }

    /// The names of the fields its conditions read.
    pub fn fields(&self, names: &mut Vec<String>) {
        for node in &self.nodes {
            match node {
                Node::Group(group) => group.fields(names),
                Node::Condition { name, .. } => {
                    if !names.contains(name) {
                        names.push(name.clone());
                    }
                }
            }
        }
    }

    /// Whether a record with `values` passes.
    pub fn keeps(&self, values: &Map<String, Value>) -> bool {
        let results = self.nodes.iter().filter_map(|node| match node {
            Node::Group(group) => Some(group.keeps(values)),
            Node::Condition { name, kind, operator, value } => condition(kind, operator, value, values.get(name)),
        });
        let results: Vec<bool> = results.collect();
        if results.is_empty() {
            return true;
        }
        if self.any { results.iter().any(|ok| *ok) } else { results.iter().all(|ok| *ok) }
    }
}

/// Whether a condition with `operator` and `value` on a field of type `kind` counts: an editor
/// condition needs an operator the editor offers for the type, and a value unless it tests for
/// emptiness.
pub fn active(kind: &str, operator: &str, value: &Value) -> bool {
    if !EDITOR_OPERATORS.contains(&operator) {
        return true;
    }
    if !editor_operators(kind).contains(&operator) {
        return false;
    }
    if matches!(operator, "isEmpty" | "isNotEmpty") {
        return true;
    }
    match value {
        Value::Null => false,
        Value::String(text) => !text.trim().is_empty(),
        Value::Array(items) => !items.is_empty(),
        _ => true,
    }
}

/// A cell's value as filters compare it.
#[derive(Clone, Debug, PartialEq)]
enum Cell {
    Empty,
    Number(f64),
    Day(i64),
    Flag(bool),
    Text(String),
    List(Vec<String>),
}

const DATE_TYPES: [&str; 4] = ["date", "datetime", "created_time", "last_modified_time"];
const NUMBER_TYPES: [&str; 3] = ["number", "rating", "auto_number"];

fn day_of(value: &Value) -> Option<i64> {
    let millis = match value {
        Value::Number(number) => number.as_f64()? as i64,
        Value::String(text) => parse_time(text)?,
        _ => return None,
    };
    Some(millis.div_euclid(86_400_000))
}

fn cell_of(kind: &str, value: Option<&Value>) -> Cell {
    let Some(value) = value.filter(|value| !value.is_null()) else {
        return if kind == "checkbox" { Cell::Flag(false) } else { Cell::Empty };
    };
    if DATE_TYPES.contains(&kind) {
        return day_of(value).map_or(Cell::Empty, Cell::Day);
    }
    if NUMBER_TYPES.contains(&kind) {
        return match value {
            Value::Number(number) => number.as_f64().map_or(Cell::Empty, Cell::Number),
            Value::String(text) => text.trim().parse::<f64>().map_or(Cell::Empty, Cell::Number),
            _ => Cell::Empty,
        };
    }
    match kind {
        "checkbox" => Cell::Flag(truthy(value)),
        "multi_select" | "user" | "created_by" | "last_modified_by" | "link_to_record" => {
            let items: Vec<String> = match value {
                Value::Array(items) => items.iter().map(|item| display(Some(item))).filter(|text| !text.is_empty()).collect(),
                other => vec![display(Some(other))],
            };
            if items.is_empty() { Cell::Empty } else { Cell::List(items) }
        }
        _ => {
            let text = display(Some(value));
            if text.trim().is_empty() { Cell::Empty } else { Cell::Text(text.trim().to_owned()) }
        }
    }
}

/// A filter value in the cell's terms.
fn value_as(cell: &Cell, value: &Value) -> Cell {
    match cell {
        Cell::Day(_) => day_of(value).map_or(Cell::Empty, Cell::Day),
        Cell::Number(_) => match value {
            Value::Number(number) => number.as_f64().map_or(Cell::Empty, Cell::Number),
            Value::String(text) => text.trim().parse::<f64>().map_or_else(|_| Cell::Text(text.clone()), Cell::Number),
            _ => Cell::Empty,
        },
        Cell::Flag(_) => Cell::Flag(truthy(value)),
        _ => match value {
            Value::Null => Cell::Empty,
            Value::Array(items) => Cell::List(items.iter().map(|item| display(Some(item))).collect()),
            other => Cell::Text(display(Some(other))),
        },
    }
}

fn texts(cell: &Cell) -> Vec<String> {
    match cell {
        Cell::Empty => Vec::new(),
        Cell::List(items) => items.clone(),
        Cell::Text(text) => vec![text.clone()],
        Cell::Number(value) => vec![number(*value)],
        Cell::Day(day) => vec![day.to_string()],
        Cell::Flag(flag) => vec![flag.to_string()],
    }
}

fn same(a: &Cell, b: &Cell) -> bool {
    match (a, b) {
        (Cell::Number(a), Cell::Number(b)) => (a - b).abs() < 1e-9,
        (Cell::List(a), Cell::Text(b)) | (Cell::Text(b), Cell::List(a)) => a.len() == 1 && a.iter().all(|item| item == b),
        _ => a == b,
    }
}

/// Whether a condition holds for a cell of a field of type `kind`; `None` when its operator is
/// unknown (it is left out).
fn condition(kind: &str, operator: &str, value: &Value, raw: Option<&Value>) -> Option<bool> {
    let cell = cell_of(kind, raw);
    let wanted = value_as(&cell, value);
    let lower = |text: &str| text.to_lowercase();
    let contains = || {
        let needle = lower(&display(Some(value)));
        texts(&cell).iter().any(|text| lower(text).contains(&needle))
    };
    let empty = matches!(cell, Cell::Empty) || matches!(&cell, Cell::List(items) if items.is_empty());
    let compare = || match (&cell, &wanted) {
        (Cell::Number(a), Cell::Number(b)) => a.partial_cmp(b),
        (Cell::Day(a), Cell::Day(b)) => Some(a.cmp(b)),
        _ => None,
    };
    let wanted_list = texts(&wanted);
    let have = texts(&cell);
    let any_of = || have.iter().any(|item| wanted_list.iter().any(|want| want.eq_ignore_ascii_case(item)));
    let all_of = || wanted_list.iter().all(|want| have.iter().any(|item| want.eq_ignore_ascii_case(item)));
    Some(match operator {
        "is" | "eq" | "equals" | "isWithIn" => same(&cell, &wanted),
        "isNot" | "neq" | "ne" | "not_equals" => !same(&cell, &wanted),
        "contains" => contains(),
        "doesNotContain" | "not_contains" | "notContains" => !contains(),
        "isEmpty" | "is_empty" | "empty" | "falsy" => empty || cell == Cell::Flag(false),
        "isNotEmpty" | "is_not_empty" | "not_empty" | "truthy" => !(empty || cell == Cell::Flag(false)),
        "isGreater" | "gt" | "greater_than" | "isAfter" => compare() == Some(Ordering::Greater),
        "isGreaterEqual" | "gte" | "isOnOrAfter" => matches!(compare(), Some(Ordering::Greater | Ordering::Equal)),
        "isLess" | "lt" | "less_than" | "isBefore" => compare() == Some(Ordering::Less),
        "isLessEqual" | "lte" | "isOnOrBefore" => matches!(compare(), Some(Ordering::Less | Ordering::Equal)),
        "isAnyOf" | "hasAnyOf" | "in" => any_of(),
        "isNoneOf" | "hasNoneOf" | "not_in" => !any_of(),
        "hasAllOf" => all_of(),
        "isExactly" => all_of() && have.len() == wanted_list.len(),
        "isNotExactly" => !(all_of() && have.len() == wanted_list.len()),
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    //! The filter assertions of web/src/editor/table/filter/evaluate.spec.ts, ported when the
    //! editor's filter moved into the engine.
    #![allow(clippy::unwrap_used)]

    use super::*;
    use serde_json::json;

    fn field(id: i64, name: &str, kind: &str) -> Field {
        Field { id, name: name.into(), title: name.into(), field_type: kind.into(), ..Field::default() }
    }

    fn fields() -> Vec<Field> {
        vec![
            field(1, "label", "text"),
            field(2, "qty", "number"),
            field(3, "due", "date"),
            field(4, "stage", "single_select"),
            field(5, "tags", "multi_select"),
            field(6, "flag", "checkbox"),
        ]
    }

    fn cond(field_name: &str, operator: &str, value: Value) -> Value {
        json!({ "id": "c1", "kind": "condition", "field_name": field_name, "operator": operator, "value": value })
    }

    fn group(children: Vec<Value>, conjunction: &str) -> Value {
        json!({ "id": "g1", "kind": "group", "conjunction": conjunction, "children": children })
    }

    /// Whether `values` pass the editor tree `tree`.
    fn keeps(tree: Value, values: Value) -> bool {
        let fields = fields();
        let tree = tree_of_config(&tree, &fields).unwrap();
        Filter::new(&tree, &fields).keeps(values.as_object().unwrap())
    }

    fn one(field_name: &str, operator: &str, value: Value, values: Value) -> bool {
        keeps(group(vec![cond(field_name, operator, value)], "and"), values)
    }

    #[test]
    fn text_operators() {
        let r = json!({ "label": "Hello world" });
        assert!(one("label", "is", json!("Hello world"), r.clone()));
        assert!(!one("label", "is", json!("nope"), r.clone()));
        // contains is case-insensitive
        assert!(one("label", "contains", json!("HELLO"), r.clone()));
        // doesNotContain inverts contains
        assert!(one("label", "doesNotContain", json!("cat"), r));
        // isEmpty matches an empty value
        assert!(one("label", "isEmpty", Value::Null, json!({ "label": "" })));
    }

    #[test]
    fn number_operators() {
        let r = json!({ "qty": 7 });
        assert!(one("qty", "is", json!(7), r.clone()));
        assert!(one("qty", "isGreater", json!(6), r.clone()));
        assert!(one("qty", "isGreaterEqual", json!(7), r.clone()));
        assert!(one("qty", "isLess", json!(8), r.clone()));
        assert!(one("qty", "isLessEqual", json!(7), r.clone()));
        assert!(!one("qty", "isGreater", json!(7), r));
    }

    #[test]
    fn date_operators_match_the_calendar_day_whatever_the_time() {
        let r = json!({ "due": "2026-06-13T18:30:00Z" });
        assert!(one("due", "is", json!("2026-06-13"), r.clone()));
        assert!(one("due", "isWithIn", json!("2026-06-13"), r.clone()));
        assert!(one("due", "isBefore", json!("2026-06-14"), r.clone()));
        assert!(one("due", "isAfter", json!("2026-06-12"), r.clone()));
        assert!(one("due", "isOnOrBefore", json!("2026-06-13"), r.clone()));
        assert!(one("due", "isOnOrAfter", json!("2026-06-13"), r.clone()));
        assert!(!one("due", "is", json!("2026-06-14"), r));
    }

    #[test]
    fn single_select_operators() {
        let r = json!({ "stage": "todo" });
        assert!(one("stage", "is", json!("todo"), r.clone()));
        assert!(one("stage", "isAnyOf", json!(["todo", "done"]), r.clone()));
        assert!(one("stage", "isNoneOf", json!(["done"]), r.clone()));
        assert!(!one("stage", "isNoneOf", json!(["todo"]), r));
    }

    #[test]
    fn multi_select_operators() {
        let r = json!({ "tags": ["red", "green"] });
        assert!(one("tags", "hasAnyOf", json!(["red"]), r.clone()));
        assert!(one("tags", "hasAllOf", json!(["red", "green"]), r.clone()));
        assert!(!one("tags", "hasAllOf", json!(["red", "blue"]), r.clone()));
        assert!(one("tags", "isExactly", json!(["green", "red"]), r.clone()));
        assert!(!one("tags", "isExactly", json!(["red"]), r.clone()));
        assert!(one("tags", "hasNoneOf", json!(["blue"]), r));
    }

    #[test]
    fn checkbox_is_true_or_false() {
        let r = json!({ "flag": true });
        assert!(one("flag", "is", json!(true), r.clone()));
        assert!(!one("flag", "is", json!(false), r));
    }

    #[test]
    fn and_requires_every_child_and_or_any() {
        let r = json!({ "label": "apple", "qty": 5 });
        let and = group(vec![cond("label", "is", json!("apple")), cond("qty", "isGreater", json!(4))], "and");
        assert!(keeps(and, r.clone()));
        let and = group(vec![cond("label", "is", json!("apple")), cond("qty", "isGreater", json!(99))], "and");
        assert!(!keeps(and, r.clone()));
        let or = group(vec![cond("label", "is", json!("pear")), cond("qty", "isGreater", json!(4))], "or");
        assert!(keeps(or, r));
    }

    #[test]
    fn conditions_without_a_value_or_field_are_left_out() {
        let r = json!({ "label": "apple", "qty": 5 });
        // inactive children are ignored
        let tree = group(vec![cond("label", "contains", json!("")), cond("qty", "isGreater", json!(4))], "and");
        assert!(keeps(tree, r.clone()));
        // ... also when the cell is empty, which a written `contains` would not keep
        assert!(one("label", "contains", json!(""), json!({})));
        assert!(one("label", "contains", Value::Null, json!({})));
        assert!(one("tags", "hasAnyOf", json!([]), json!({})));
        // an unknown field
        assert!(one("nope", "is", json!("x"), r.clone()));
        // isEmpty needs no value
        assert!(!one("label", "isEmpty", Value::Null, r.clone()));
        // a written condition counts
        assert!(!one("label", "contains", json!("x"), r));
    }

    // isConditionActive's assertions.
    #[test]
    fn an_editor_condition_counts_with_an_offered_operator_and_a_value() {
        // an operator the editor does not offer for the field's type
        assert!(!active("text", "hasAnyOf", &json!(["x"])));
        assert!(one("label", "hasAnyOf", json!(["x"]), json!({ "label": "y" })));
        // isEmpty needs no value
        assert!(active("text", "isEmpty", &Value::Null));
        // no value: nothing, blank text, an empty list
        assert!(!active("text", "contains", &Value::Null));
        assert!(!active("text", "contains", &json!("")));
        assert!(!active("multi_select", "hasAnyOf", &json!([])));
        assert!(active("text", "contains", &json!("x")));
        // the editor filters no formula; the engine's operators filter any field
        assert!(!active("formula", "is", &json!("x")));
        assert!(active("formula", "eq", &json!("x")));
    }

    #[test]
    fn an_empty_group_keeps_every_record() {
        assert!(keeps(group(Vec::new(), "and"), json!({ "label": "apple" })));
    }

    #[test]
    fn nested_or_inside_and() {
        let r = json!({ "label": "apple", "qty": 5 });
        let tree =
            group(vec![cond("label", "is", json!("apple")), group(vec![cond("qty", "is", json!(1)), cond("qty", "is", json!(5))], "or")], "and");
        assert!(keeps(tree, r));
    }

    #[test]
    fn engine_operators_filter_as_the_engine_writes_them() {
        assert!(one("stage", "neq", json!("done"), json!({ "stage": "todo" })));
        assert!(!one("stage", "eq", json!("done"), json!({ "stage": "todo" })));
        assert!(one("qty", "gt", json!("4"), json!({ "qty": 5 })));
        // an engine condition with no value tests for an empty cell
        assert!(one("label", "eq", Value::Null, json!({})));
        // an operator nobody knows is left out
        assert!(one("label", "matches", json!("x"), json!({ "label": "y" })));
    }

    #[test]
    fn a_tree_reads_fields_by_name_then_id_and_counts_its_conditions() {
        let fields = fields();
        let tree = tree_of_config(
            &json!({ "kind": "group", "conjunction": "or", "children": [
                { "kind": "condition", "field_name": "qty", "operator": "is", "value": 1 },
                { "kind": "condition", "field_id": 4, "operator": "is", "value": "x" },
                { "kind": "group", "conjunction": "and", "children": [{ "kind": "condition", "field_name": "gone", "operator": "is", "value": 1 }] },
            ] }),
            &fields,
        )
        .unwrap();
        assert_eq!(tree.conjunction, "or");
        assert_eq!(count_conditions(&tree), 3);
        let ids: Vec<i64> = tree.children.iter().filter_map(|child| child.condition.as_ref().map(|rule| rule.field_id)).collect();
        assert_eq!(ids, vec![2, 4]);
        assert!(tree_of_config(&json!({ "kind": "condition" }), &fields).is_none());
    }
}
