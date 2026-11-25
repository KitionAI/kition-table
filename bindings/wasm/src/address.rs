//! Tool UUIDs at the command boundary; projection integers remain local addresses.

use kition_kit::{KitCommand, Within};
use kition_table_file::TableFile;
use serde_json::{Value, json};

pub fn row(ty: &str) -> Option<(&'static str, &'static str)> {
    Some(match ty {
        "Table" => ("kition_tables", "kdm_id"),
        "Field" => ("kition_fields", "kdm_id"),
        "View" => ("kition_views", "kdm_id"),
        "Record" => ("kition_records", "row_key"),
        _ => return None,
    })
}

pub fn local(file: &TableFile, ty: &str, value: &Value) -> Result<i64, String> {
    if let Some(id) = value.as_i64().or_else(|| value.as_str()?.parse().ok()) {
        return Ok(id);
    }
    let (table, column) = row(ty).ok_or_else(|| format!("no local address for {ty}"))?;
    let id = value.as_str().ok_or_else(|| format!("{ty} address must be a tool UUID or local integer"))?;
    file.conn()
        .query_row(&format!("SELECT id FROM {table} WHERE {column} = ?1 ORDER BY deleted_at IS NOT NULL, id DESC LIMIT 1"), [id], |r| r.get(0))
        .map_err(|_| format!("not found: {ty} {id}"))
}

fn visit(value: &mut Value, path: &[&str], f: &mut impl FnMut(&mut Value) -> Result<(), String>) -> Result<(), String> {
    let Some((head, tail)) = path.split_first() else {
        if let Some(items) = value.as_array_mut() {
            for item in items {
                f(item)?;
            }
        } else if !value.is_null() {
            f(value)?;
        }
        return Ok(());
    };
    let (key, array) = head.strip_suffix("[]").map_or((*head, false), |key| (key, true));
    if let Some(value) = value.get_mut(key) {
        if array {
            for item in value.as_array_mut().into_iter().flatten() {
                visit(item, tail, f)?;
            }
        } else {
            visit(value, tail, f)?;
        }
    }
    Ok(())
}

pub fn normalize(file: &TableFile, command: &KitCommand, params: &Value) -> Result<Value, String> {
    let mut out = params.clone();
    for (path, types) in command.object_types {
        if command.name_params.iter().any(|(name, _)| name == path) {
            continue;
        }
        let Some(ty) = types.first().filter(|ty| row(ty).is_some()) else { continue };
        let key = path.rsplit('.').next().unwrap_or(path);
        let record_key = matches!(key, "rowKey" | "rowKeys" | "row_keys" | "target_row_keys" | "before" | "after");
        visit(&mut out, &path.split('.').collect::<Vec<_>>(), &mut |value| {
            if record_key {
                if value.as_i64().is_some() || value.as_str().is_some_and(|s| s.parse::<i64>().is_ok()) {
                    *value = json!(crate::place::resolve(file, ty, value).ok_or_else(|| format!("not found: {ty} {value}"))?);
                }
            } else {
                *value = json!(local(file, ty, value)?);
            }
            Ok(())
        })?;
    }
    fn nested(file: &TableFile, value: &mut Value) -> Result<(), String> {
        match value {
            Value::Array(items) => {
                for item in items {
                    nested(file, item)?;
                }
            }
            Value::Object(map) => {
                for (key, value) in map {
                    if let Some(ty) = crate::params::kind(key).and_then(|types| types.first()).filter(|ty| row(ty).is_some()) {
                        if !value.is_null() {
                            *value = json!(local(file, ty, value)?);
                        }
                    } else {
                        nested(file, value)?;
                    }
                }
            }
            _ => {}
        }
        Ok(())
    }
    for key in ["config", "tree", "items", "widgets", "options", "field_overrides"] {
        if let Some(value) = out.get_mut(key) {
            nested(file, value)?;
        }
    }
    Ok(out)
}

pub fn windows(file: &TableFile, command: &KitCommand, params: &Value) -> Vec<Within> {
    let mut windows = Vec::new();
    for (path, types) in command.object_types {
        for value in crate::params::values(params, path) {
            for ty in *types {
                windows.extend(window(file, ty, value));
            }
        }
    }
    fn nested(file: &TableFile, value: &Value, windows: &mut Vec<Within>) {
        match value {
            Value::Array(items) => {
                for item in items {
                    nested(file, item, windows);
                }
            }
            Value::Object(map) => {
                for (key, value) in map {
                    if let Some(types) = crate::params::kind(key) {
                        for ty in types {
                            for value in value.as_array().map(Vec::as_slice).unwrap_or(std::slice::from_ref(value)) {
                                windows.extend(window(file, ty, value));
                            }
                        }
                    } else {
                        nested(file, value, windows);
                    }
                }
            }
            _ => {}
        }
    }
    // Capture positional spelling before normalizing UUIDs to projection integers.
    for key in ["config", "tree", "items", "widgets", "options", "field_overrides"] {
        if let Some(value) = params.get(key) {
            nested(file, value, &mut windows);
        }
    }
    let mut unique = Vec::new();
    for window in windows {
        if !unique.contains(&window) {
            unique.push(window);
        }
    }
    unique
}

pub(crate) fn window(file: &TableFile, ty: &str, value: &Value) -> Vec<Within> {
    if value.as_i64().or_else(|| value.as_str()?.parse().ok()).is_none() {
        return Vec::new();
    }
    let Some(id) = crate::place::resolve(file, ty, value) else { return Vec::new() };
    let list = match ty {
        "Table" => "tables",
        "Field" => "fields",
        "View" => "views",
        "Record" => "records",
        _ => return Vec::new(),
    };
    file.replica()
        .objects_of_type(if ty == "Table" { "Base" } else { "Table" })
        .into_iter()
        .filter_map(|parent| {
            let items = file.replica().list(&parent, list).ok()?;
            let at = items.iter().position(|item| item.as_str() == id)?;
            Some(Within { parent: json!(parent.as_str()), field: list.into(), from: at, to: at + 1, ..Default::default() })
        })
        .collect()
}

pub fn schema(node: &mut Value) {
    if let Some(items) = node.get_mut("items") {
        schema(items);
    }
    if let Some(props) = node.get_mut("properties").and_then(Value::as_object_mut) {
        for (key, value) in props {
            if crate::params::kind(key).is_some() {
                id_schema(value);
            } else {
                schema(value);
            }
        }
    }
    for branch in ["oneOf", "anyOf"] {
        for node in node.get_mut(branch).and_then(Value::as_array_mut).into_iter().flatten() {
            schema(node);
        }
    }
}

pub fn id_schema(node: &mut Value) {
    if node["type"] == "array" {
        id_schema(&mut node["items"]);
        return;
    }
    if node["type"] == "integer" || node["type"] == "string" {
        node.as_object_mut().map(|node| node.remove("type"));
        node["oneOf"] = json!([{"type":"integer"},{"type":"string"}]);
    }
    schema(node);
}

pub fn tool(file: &TableFile, ty: &str, value: &Value) -> Value {
    crate::place::resolve(file, ty, value).map_or(Value::Null, Value::String)
}

pub fn label(file: &TableFile, ty: &str, value: &mut Value) {
    if value.get("id").is_some() {
        value["tool_id"] = if ty == "Base" { file.base_object().map_or(Value::Null, |id| json!(id.as_str())) } else { tool(file, ty, &value["id"]) };
    }
}

pub fn answer(file: &TableFile, method: &str, value: &mut Value) {
    if method == "record.history" {
        return;
    }
    let ty = match method.split('.').next() {
        _ if method == "file.get" => "Base",
        Some("table") => "Table",
        Some("field") => "Field",
        Some("tableView") => "View",
        Some("record") => "Record",
        _ => return,
    };
    fn object(file: &TableFile, ty: &str, value: &mut Value) {
        label(file, ty, value);
        for (key, ty) in [("fields", "Field"), ("views", "View"), ("tables", "Table")] {
            for item in value.get_mut(key).and_then(Value::as_array_mut).into_iter().flatten() {
                object(file, ty, item);
            }
        }
    }
    object(file, ty, value);
    for item in value.get_mut("items").and_then(Value::as_array_mut).into_iter().flatten() {
        object(file, ty, item);
    }
}
