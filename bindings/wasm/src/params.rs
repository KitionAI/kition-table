//! Object parameters audited against api::dispatch, kdm::options, transact and ai-field.
//! Projection integers are resolved to persistent replica ids by place::under.

use std::collections::HashMap;
use std::sync::OnceLock;

use kition_kit::KitCommand;
use serde_json::Value;

type Types = &'static [(&'static str, &'static [&'static str])];
type Names = &'static [(&'static str, &'static str)];
type Declaration = (&'static [&'static str], Types, Names);
type Roles = (&'static [&'static str], &'static [&'static str]);

pub(crate) fn kind(path: &str) -> Option<&'static [&'static str]> {
    Some(match path.rsplit('.').next()? {
        "tableId" | "table_id" | "target_table_id" | "source_table_id" | "sourceTableId" => &["Table"],
        "fieldId" | "fieldIds" | "field_id" | "primary_field_id" | "primaryFieldId" | "symmetric_field_id" | "link_field_id" | "target_field_id"
        | "source_field_id" => &["Field"],
        "viewId" | "view_id" => &["View"],
        "recordId" | "record_ids" | "rowKey" | "rowKeys" | "row_keys" | "target_row_keys" | "before" | "after" => &["Record"],
        "dashboardId" => &["Dashboard"],
        _ => return None,
    })
}

fn walk(node: &Value, path: &str, found: &mut Vec<(String, &'static [&'static str])>) {
    if let Some(types) = kind(path) {
        found.push((path.to_owned(), types));
    }
    if let Some(props) = node.get("properties").and_then(Value::as_object) {
        for (key, node) in props {
            walk(node, &format!("{path}.{key}"), found);
        }
    }
    if let Some(items) = node.get("items").filter(|n| n.get("properties").is_some()) {
        walk(items, &format!("{path}[]"), found);
    }
    for key in ["oneOf", "anyOf"] {
        for branch in node.get(key).and_then(Value::as_array).into_iter().flatten() {
            walk(branch, path, found);
        }
    }
}

pub fn declare(commands: &mut [KitCommand]) {
    for command in commands.iter_mut() {
        command.index_numbers = true;
    }
    static DECLARED: OnceLock<HashMap<String, Declaration>> = OnceLock::new();
    let declared = DECLARED.get_or_init(|| {
        commands
            .iter()
            .map(|command| {
                let mut found = Vec::new();
                for (key, node) in command.schema.as_ref().and_then(Value::as_object).into_iter().flatten() {
                    walk(node, key, &mut found);
                }
                // These structured parameters are intentionally open JSON. Mark the actual references,
                // never the entire object: counts, values and coordinates are not ids.
                if command.id == "edit.transact" {
                    for (path, types) in [
                        ("ops[].tableId", &["Table"][..]),
                        ("ops[].fieldIds", &["Field"][..]),
                        ("ops[].records[].rowKey", &["Record"][..]),
                        ("ops[].rowKeys", &["Record"][..]),
                        ("ops[].before", &["Record"][..]),
                        ("ops[].at.before", &["Record"][..]),
                        ("ops[].at.after", &["Record"][..]),
                    ] {
                        found.push((path.to_owned(), types));
                    }
                }
                if matches!(command.id.as_str(), "ai.apply" | "ai.prepare") {
                    found.push(("config.source_field_id".into(), &["Field"]));
                }
                if command.id == "file.runImport" {
                    for (path, types) in [
                        ("field_overrides[].options.target_table_id", &["Table"][..]),
                        ("field_overrides[].options.link_field_id", &["Field"][..]),
                        ("field_overrides[].options.target_field_id", &["Field"][..]),
                        ("field_overrides[].options.symmetric_field_id", &["Field"][..]),
                        ("field_overrides[].options.ai_config.source_field_id", &["Field"][..]),
                    ] {
                        found.push((path.into(), types));
                    }
                }
                found.sort_unstable_by(|a, b| a.0.cmp(&b.0));
                found.dedup_by(|a, b| a.0 == b.0);
                let types: Types =
                    Box::leak(found.into_iter().map(|(p, t)| (&*Box::leak(p.into_boxed_str()), t)).collect::<Vec<_>>().into_boxed_slice());
                let objects =
                    Box::leak(types.iter().filter_map(|(p, _)| (Some(*p) != command.targets).then_some(*p)).collect::<Vec<_>>().into_boxed_slice());
                let names: Names = match command.id.as_str() {
                    "field.add" | "field.set" => &[("options.ai_config.source_field_name", "name")],
                    "ai.apply" | "ai.prepare" => &[("config.source_field_name", "name")],
                    "file.runImport" => &[("field_overrides[].options.ai_config.source_field_name", "name")],
                    _ => &[],
                };
                let types: Types = if names.is_empty() {
                    types
                } else {
                    Box::leak(
                        types.iter().copied().chain(names.iter().map(|(path, _)| (*path, &["Field"][..]))).collect::<Vec<_>>().into_boxed_slice(),
                    )
                };
                (command.id.clone(), (&*objects, types, names))
            })
            .collect()
    });
    for command in commands.iter_mut() {
        if let Some((objects, types, names)) = declared.get(&command.id) {
            command.objects = objects;
            command.object_types = types;
            command.name_params = names;
        }
    }
    static ROLES: OnceLock<HashMap<String, Roles>> = OnceLock::new();
    let roles = ROLES.get_or_init(|| {
        commands
            .iter()
            .map(|command| {
                let places = places(command);
                // The SDK publishes one role per parameter; object would overwrite place.
                let objects: &'static [&'static str] =
                    Box::leak(command.objects.iter().copied().filter(|path| !places.contains(path)).collect::<Vec<_>>().into_boxed_slice());
                (command.id.clone(), (places, objects))
            })
            .collect()
    });
    for command in commands {
        if let Some((places, objects)) = roles.get(&command.id) {
            command.places = places;
            command.objects = objects;
        }
    }
}

fn places(command: &KitCommand) -> &'static [&'static str] {
    let mut paths = crate::place::places(&command.id).to_vec();
    paths.extend(
        command
            .object_types
            .iter()
            .filter(|(path, _)| Some(*path) != command.targets && *path != "viewId")
            .filter_map(|(path, _)| path.split('.').next())
            .map(|p| p.trim_end_matches("[]")),
    );
    paths.sort_unstable();
    paths.dedup();
    Box::leak(paths.into_boxed_slice())
}

pub fn values<'a>(params: &'a Value, path: &str) -> Vec<&'a Value> {
    let mut values = vec![params];
    for segment in path.split('.') {
        let (key, array) = segment.strip_suffix("[]").map_or((segment, false), |s| (s, true));
        values = values
            .into_iter()
            .filter_map(|v| v.get(key))
            .flat_map(|v| if array { v.as_array().map(|v| v.iter().collect()).unwrap_or_default() } else { vec![v] })
            .collect();
    }
    values.into_iter().flat_map(|v| v.as_array().map(|a| a.iter().collect()).unwrap_or_else(|| vec![v])).collect()
}
