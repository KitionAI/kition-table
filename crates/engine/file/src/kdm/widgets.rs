//! A dashboard's widgets in the document.
//!
//! Writers give a dashboard as one config (`order`, `source_table`, `layout`: a list of
//! `{widget_id, x, y, w, h, ...}`) and a map of widgets by widget id. The document keeps the
//! widgets as `Widget` objects in `Dashboard.widgets`, each with its key (the widget id), `title`,
//! `type`, `query`, the rest of its settings (`settings`) and its place on the dashboard
//! (`layout`, which changes as a unit), so two people adding or changing different widgets at once
//! both keep their change. What stays in `Dashboard.config` (`order`, `source_table`) changes as a
//! unit. A widget's query names its fields by object id ([`query_to_kdm`]); the dashboard's own
//! table is `source_table`.

use std::collections::HashMap;

use kition_kdm::{ObjectId, Replica, Tx};
use serde_json::{Map, Value, json};

use super::model::{Listing, place_items};
use crate::Result;

const OWN: [&str; 4] = ["id", "title", "type", "query"];

/// A widget's query with its fields by object id: `field_name` becomes `field`,
/// `group_by_field_name` `group_by_field`, each filter's `field_name` its `field`, and `columns`
/// (names) the fields' ids. A name that names no field is `null` (a column left out).
pub fn query_to_kdm(query: &Value, field_of_name: &dyn Fn(&str) -> Option<ObjectId>) -> Value {
    let Value::Object(map) = query else {
        return query.clone();
    };
    let mut out = map.clone();
    let id = |name: &Value| name.as_str().and_then(field_of_name).map_or(Value::Null, |id| json!(id.as_str()));
    for (named, kdm) in [("field_name", "field"), ("group_by_field_name", "group_by_field")] {
        if let Some(name) = out.remove(named) {
            out.insert(kdm.into(), id(&name));
        }
    }
    if let Some(Value::Array(filters)) = out.get_mut("filters") {
        for filter in filters {
            if let Value::Object(filter) = filter
                && let Some(name) = filter.remove("field_name")
            {
                filter.insert("field".into(), id(&name));
            }
        }
    }
    if let Some(Value::Array(columns)) = out.get_mut("columns") {
        *columns = columns.iter().filter_map(Value::as_str).filter_map(field_of_name).map(|id| json!(id.as_str())).collect();
    }
    Value::Object(out)
}

/// [`query_to_kdm`] in reverse: `name_of` answers a field object's name; a field it does not
/// know is left out.
pub fn query_from_kdm(query: &Value, name_of: &dyn Fn(&ObjectId) -> Option<String>) -> Value {
    let Value::Object(map) = query else {
        return query.clone();
    };
    let mut out = map.clone();
    let name = |id: &Value| id.as_str().and_then(|id| name_of(&ObjectId::from_string(id)));
    for (named, kdm) in [("field_name", "field"), ("group_by_field_name", "group_by_field")] {
        if let Some(id) = out.remove(kdm)
            && let Some(found) = name(&id)
        {
            out.insert(named.into(), json!(found));
        }
    }
    if let Some(Value::Array(filters)) = out.get_mut("filters") {
        for filter in filters {
            if let Value::Object(filter) = filter
                && let Some(id) = filter.remove("field")
                && let Some(found) = name(&id)
            {
                filter.insert("field_name".into(), json!(found));
            }
        }
    }
    if let Some(Value::Array(columns)) = out.get_mut("columns") {
        *columns = columns.iter().filter_map(|id| name(id)).map(|found| json!(found)).collect();
    }
    Value::Object(out)
}

/// The registers of a widget written as `widget` (its JSON) placed at `layout` (its layout entry
/// without `widget_id`, `null`: none).
fn registers(key: &str, widget: &Value, layout: &Value) -> Map<String, Value> {
    let mut values = Map::new();
    values.insert("key".into(), json!(key));
    for name in ["title", "type", "query"] {
        if let Some(value) = widget.get(name).filter(|value| !value.is_null()) {
            values.insert(name.into(), value.clone());
        }
    }
    let settings: Map<String, Value> = widget
        .as_object()
        .into_iter()
        .flatten()
        .filter(|(name, _)| !OWN.contains(&name.as_str()))
        .map(|(name, value)| (name.clone(), value.clone()))
        .collect();
    if !settings.is_empty() {
        values.insert("settings".into(), Value::Object(settings));
    }
    if !layout.is_null() {
        values.insert("layout".into(), layout.clone());
    }
    values
}

/// Splits a dashboard config as writers give it into what stays in `Dashboard.config` and the
/// layout entries by widget id, in their order.
pub fn split_layout(config: &Value) -> (Value, Vec<(String, Value)>) {
    let Value::Object(map) = config else {
        return (config.clone(), Vec::new());
    };
    let mut rest = map.clone();
    let layout = match rest.remove("layout") {
        Some(Value::Array(items)) => items
            .into_iter()
            .filter_map(|item| {
                let mut item = item.as_object()?.clone();
                let key = item.remove("widget_id")?.as_str()?.to_owned();
                Some((key, Value::Object(item)))
            })
            .collect(),
        _ => Vec::new(),
    };
    if map.contains_key("layout") {
        // That the dashboard has a layout (the widgets hold its entries).
        rest.insert("layout".into(), json!([]));
    }
    (Value::Object(rest), layout)
}

/// Makes `Dashboard.widgets` of `dashboard` hold `widgets` (by widget id; `null` ones are left
/// out) placed by `layout`: a widget with the same key is kept and written where it differs, the
/// others are new, and the ones not given are deleted. The list follows the layout's order, then
/// the widgets it does not place, by key.
pub fn store(tx: &mut Tx<'_>, dashboard: &ObjectId, widgets: &Map<String, Value>, layout: &[(String, Value)]) -> Result<()> {
    let placed: HashMap<&str, &Value> = layout.iter().map(|(key, item)| (key.as_str(), item)).collect();
    let mut keys: Vec<&String> = Vec::new();
    for (key, _) in layout {
        if let Some((known, widget)) = widgets.get_key_value(key)
            && !widget.is_null()
            && !keys.contains(&known)
        {
            keys.push(known);
        }
    }
    let mut rest: Vec<&String> = widgets.iter().filter(|(key, widget)| !widget.is_null() && !keys.contains(key)).map(|(key, _)| key).collect();
    rest.sort();
    keys.extend(rest);
    let existing = Listing::read(tx, dashboard, "widgets")?.ids();
    let mut by_key: HashMap<String, ObjectId> = HashMap::new();
    for id in &existing {
        if let Some(key) = tx.get(id, "key")?.as_str() {
            by_key.entry(key.to_owned()).or_insert_with(|| id.clone());
        }
    }
    let mut order = Vec::with_capacity(keys.len());
    for key in keys {
        let widget = widgets.get(key).unwrap_or(&Value::Null);
        let layout = placed.get(key.as_str()).copied().cloned().unwrap_or(Value::Null);
        let wanted = registers(key, widget, &layout);
        let id = match by_key.get(key) {
            Some(id) => {
                for name in ["title", "type", "query", "settings", "layout"] {
                    let value = wanted.get(name).cloned().unwrap_or(Value::Null);
                    if !crate::json::equal(&tx.get(id, name)?, &value) {
                        tx.set(id, name, value)?;
                    }
                }
                id.clone()
            }
            None => {
                let id = tx.create_with_id("Widget", &crate::ids::object(), wanted)?;
                let len = tx.list_items(dashboard, "widgets")?.len();
                tx.list_insert(dashboard, "widgets", len, &id)?;
                id
            }
        };
        order.push(id);
    }
    let items = tx.list_items(dashboard, "widgets")?;
    let mut gone = Vec::new();
    for (index, item) in items.iter().enumerate().rev() {
        if !order.contains(item) {
            tx.list_remove(dashboard, "widgets", index)?;
            if !gone.contains(item) {
                gone.push(item.clone());
            }
        }
    }
    for item in gone {
        if tx.exists(&item) {
            tx.delete(&item)?;
        }
    }
    let raw = tx.list_items(dashboard, "widgets")?;
    place_items(tx, dashboard, "widgets", raw, &order, None)
}

/// A dashboard's config and widgets as writers gave them ([`store`] in reverse, fields still by
/// object id): the config with its `layout` (the widgets' places, in the list's order) and the
/// widgets by key.
pub fn assemble(replica: &Replica, dashboard: &ObjectId) -> Result<(Value, Map<String, Value>)> {
    let mut config = replica.get(dashboard, "config")?;
    let mut seen = std::collections::HashSet::new();
    let mut layout = Vec::new();
    let mut widgets = Map::new();
    for id in replica.list(dashboard, "widgets")? {
        if !replica.exists(&id) || !seen.insert(id.clone()) {
            continue;
        }
        let Some(key) = replica.get(&id, "key")?.as_str().map(str::to_owned) else { continue };
        if widgets.contains_key(&key) {
            continue;
        }
        let mut widget = Map::new();
        widget.insert("id".into(), json!(key));
        for name in ["title", "type", "query"] {
            let value = replica.get(&id, name)?;
            if !value.is_null() {
                widget.insert(name.into(), value);
            }
        }
        if let Value::Object(settings) = replica.get(&id, "settings")? {
            widget.extend(settings);
        }
        if let Value::Object(place) = replica.get(&id, "layout")? {
            let mut item = Map::new();
            item.insert("widget_id".into(), json!(key));
            item.extend(place);
            layout.push(Value::Object(item));
        }
        widgets.insert(key, Value::Object(widget));
    }
    if let Value::Object(map) = &mut config {
        if map.contains_key("layout") || !layout.is_empty() {
            map.insert("layout".into(), Value::Array(layout));
        }
    } else if !layout.is_empty() {
        config = json!({ "layout": layout });
    }
    Ok((config, widgets))
}
