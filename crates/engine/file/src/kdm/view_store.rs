//! Where a view's settings live in the document.
//!
//! A view config in the document's form ([`super::view_refs`]: fields by object id) is one JSON
//! value to readers and writers, but the document keeps each collection in it as a container, so
//! two people changing it at once both keep their change:
//!
//! - `filter_tree` is `View.filters`: a list of `Filter` objects, groups and conditions, each
//!   naming its group (`parent`; none: the root group) and, a condition, its field by object id.
//!   Two filters added at once are both kept.
//! - `sorts` and `groups` (the API's) are `View.sorts` and `View.groups`, `sort_items` and
//!   `group_items` (the editor's) `View.sortItems` and `View.groupItems`: lists of `Sort` and
//!   `Group` objects (`{key, field, direction}`), so sorts added and reordered at once merge.
//! - `hidden_fields` is the map `View.hiddenFields` (field id to its place in the list) and
//!   `field_layout` the map `View.layout` (field id to `{position, frozen}`): one entry per field.
//!
//! What stays in the `config` register changes as a unit: `row_height`, `frozen_column_count`,
//! the editor's single `sort` and `filter`, `group_field`, `cover_field`, and keys the kit does not
//! know.
//!
//! [`store`] writes a config (a write replaces the whole config: a collection it leaves out is
//! emptied, as before), changing only the items that differ, and [`assemble`] reads it back.

use std::collections::HashMap;

use kition_kdm::{ObjectId, Replica, Tx};
use serde_json::{Map, Value, json};

use super::model::{Listing, place_items};
use crate::Result;

/// `(config key, View list field, item type)`.
const LISTS: [(&str, &str, &str); 4] =
    [("sorts", "sorts", "Sort"), ("groups", "groups", "Group"), ("sort_items", "sortItems", "Sort"), ("group_items", "groupItems", "Group")];
const FILTER_TREE: &str = "filter_tree";
const HIDDEN: (&str, &str) = ("hidden_fields", "hiddenFields");
const LAYOUT: (&str, &str) = ("field_layout", "layout");

/// The config keys that are containers of the view, not part of its `config` register.
pub fn is_container_key(key: &str) -> bool {
    key == FILTER_TREE || key == HIDDEN.0 || key == LAYOUT.0 || LISTS.iter().any(|(config, ..)| *config == key)
}

/// Writes `config` (the document's form) as `view`'s settings: the collections into their
/// containers, the rest into the `config` register (`null` when nothing is left).
pub fn store(tx: &mut Tx<'_>, view: &ObjectId, config: &Value) -> Result<()> {
    let map = config.as_object().cloned().unwrap_or_default();
    let rest: Map<String, Value> = map.iter().filter(|(key, _)| !is_container_key(key)).map(|(key, value)| (key.clone(), value.clone())).collect();
    let rest = if rest.is_empty() && !config.is_object() { Value::Null } else { Value::Object(rest) };
    let current = tx.get(view, "config")?;
    let unchanged = crate::json::equal(&current, &rest) || (current.is_null() && rest.as_object().is_some_and(Map::is_empty));
    if !unchanged {
        tx.set(view, "config", rest)?;
    }
    for (key, field, ty) in LISTS {
        let wanted: Vec<Value> = map.get(key).and_then(Value::as_array).cloned().unwrap_or_default();
        store_orderings(tx, view, field, ty, &wanted)?;
    }
    store_filters(tx, view, map.get(FILTER_TREE).unwrap_or(&Value::Null))?;
    let hidden: Map<String, Value> = map
        .get(HIDDEN.0)
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .enumerate()
        .map(|(index, id)| (id.to_owned(), json!(index)))
        .collect();
    store_map(tx, view, HIDDEN.1, &hidden)?;
    let layout = map.get(LAYOUT.0).and_then(Value::as_object).cloned().unwrap_or_default();
    store_map(tx, view, LAYOUT.1, &layout)?;
    Ok(())
}

/// Makes the map `field` of `view` hold `wanted`, key by key.
fn store_map(tx: &mut Tx<'_>, view: &ObjectId, field: &str, wanted: &Map<String, Value>) -> Result<()> {
    for (key, _) in tx.map_entries(view, field)? {
        if !wanted.contains_key(&key) {
            tx.map_delete(view, field, &key)?;
        }
    }
    for (key, value) in wanted {
        if !crate::json::equal(&tx.map_get(view, field, key)?, value) {
            tx.map_set(view, field, key, value.clone())?;
        }
    }
    Ok(())
}

fn text(value: &Value, key: &str) -> Value {
    value.get(key).filter(|value| !value.is_null()).cloned().unwrap_or(Value::Null)
}

/// One sort or group as stored: its key, field and direction.
fn ordering_of(value: &Value) -> (Value, Value, String) {
    (text(value, "id"), text(value, "field"), value.get("direction").and_then(Value::as_str).unwrap_or("asc").to_owned())
}

fn stored_ordering(tx: &Tx<'_>, id: &ObjectId) -> Result<(Value, Value, String)> {
    Ok((tx.get(id, "key")?, tx.get(id, "field")?, tx.get(id, "direction")?.as_str().unwrap_or("asc").to_owned()))
}

/// Makes the list `field` of `view` hold `wanted` (`[{id?, field, direction}]`) in order: an item
/// with the same key, else the same field and direction, is kept (and written where it differs),
/// the others are new; the items no longer wanted are deleted.
fn store_orderings(tx: &mut Tx<'_>, view: &ObjectId, field: &str, ty: &str, wanted: &[Value]) -> Result<()> {
    let existing = Listing::read(tx, view, field)?.ids();
    let stored: Vec<(Value, Value, String)> = existing.iter().map(|id| stored_ordering(tx, id)).collect::<Result<_>>()?;
    let mut taken = vec![false; existing.len()];
    let mut chosen: Vec<Option<ObjectId>> = vec![None; wanted.len()];
    for (slot, item) in chosen.iter_mut().zip(wanted) {
        let (key, _, _) = ordering_of(item);
        if key.is_null() {
            continue;
        }
        if let Some(at) = (0..existing.len()).find(|at| !taken[*at] && stored[*at].0 == key) {
            taken[at] = true;
            *slot = Some(existing[at].clone());
        }
    }
    for (slot, item) in chosen.iter_mut().zip(wanted) {
        if slot.is_some() {
            continue;
        }
        let (_, wanted_field, direction) = ordering_of(item);
        if let Some(at) = (0..existing.len()).find(|at| !taken[*at] && stored[*at].1 == wanted_field && stored[*at].2 == direction) {
            taken[at] = true;
            *slot = Some(existing[at].clone());
        }
    }
    let mut order = Vec::with_capacity(wanted.len());
    for (slot, item) in chosen.into_iter().zip(wanted) {
        let (key, wanted_field, direction) = ordering_of(item);
        let id = match slot {
            Some(id) => {
                for (name, value) in [("key", key), ("field", wanted_field), ("direction", json!(direction))] {
                    if tx.get(&id, name)? != value {
                        tx.set(&id, name, value)?;
                    }
                }
                id
            }
            None => {
                let mut values = Map::new();
                values.insert("direction".into(), json!(direction));
                if !key.is_null() {
                    values.insert("key".into(), key);
                }
                if !wanted_field.is_null() {
                    values.insert("field".into(), wanted_field);
                }
                let id = tx.create_with_id(ty, &crate::ids::object(), values)?;
                let len = tx.list_items(view, field)?.len();
                tx.list_insert(view, field, len, &id)?;
                id
            }
        };
        order.push(id);
    }
    drop_unwanted(tx, view, field, &order)?;
    let raw = tx.list_items(view, field)?;
    place_items(tx, view, field, raw, &order, None)
}

/// Removes from the list `field` of `view` every item `order` does not hold, and deletes it.
fn drop_unwanted(tx: &mut Tx<'_>, view: &ObjectId, field: &str, order: &[ObjectId]) -> Result<()> {
    let items = tx.list_items(view, field)?;
    let mut gone = Vec::new();
    for (index, item) in items.iter().enumerate().rev() {
        if !order.contains(item) {
            tx.list_remove(view, field, index)?;
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
    Ok(())
}

/// A filter node wanted, in pre-order: its key, kind, parent (index of the wanted group) and its
/// registers.
struct WantedFilter {
    key: Value,
    kind: String,
    parent: Option<usize>,
    conjunction: Value,
    field: Value,
    operator: Value,
    value: Value,
}

fn flatten(node: &Value, parent: Option<usize>, out: &mut Vec<WantedFilter>) {
    let kind = if node.get("kind").and_then(Value::as_str) == Some("condition") { "condition" } else { "group" };
    let index = out.len();
    out.push(WantedFilter {
        key: text(node, "id"),
        kind: kind.to_owned(),
        parent,
        conjunction: if kind == "group" { text(node, "conjunction") } else { Value::Null },
        field: if kind == "condition" { text(node, "field") } else { Value::Null },
        operator: if kind == "condition" { text(node, "operator") } else { Value::Null },
        value: if kind == "condition" { node.get("value").cloned().unwrap_or(Value::Null) } else { Value::Null },
    });
    if kind == "group" {
        for child in node.get("children").and_then(Value::as_array).into_iter().flatten() {
            flatten(child, Some(index), out);
        }
    }
}

/// Makes `View.filters` hold the tree `tree` (`null`: none): a node with the same key, else the
/// same content under the same group, is kept; the others are new; the rest are deleted.
fn store_filters(tx: &mut Tx<'_>, view: &ObjectId, tree: &Value) -> Result<()> {
    let mut wanted = Vec::new();
    if tree.get("kind").and_then(Value::as_str) == Some("group") {
        flatten(tree, None, &mut wanted);
    }
    let existing = Listing::read(tx, view, "filters")?.ids();
    let get = |tx: &Tx<'_>, id: &ObjectId, name: &str| -> Result<Value> { Ok(tx.get(id, name)?) };
    let mut taken = vec![false; existing.len()];
    let mut chosen: Vec<Option<ObjectId>> = vec![None; wanted.len()];
    for (slot, node) in chosen.iter_mut().zip(&wanted) {
        if node.key.is_null() {
            continue;
        }
        for (at, id) in existing.iter().enumerate() {
            if !taken[at] && get(tx, id, "key")? == node.key && get(tx, id, "kind")? == json!(node.kind) {
                taken[at] = true;
                *slot = Some(id.clone());
                break;
            }
        }
    }
    for (index, node) in wanted.iter().enumerate() {
        if chosen[index].is_some() {
            continue;
        }
        let parent = node.parent.and_then(|parent| chosen[parent].clone()).map_or(Value::Null, |id| json!(id.as_str()));
        for (at, id) in existing.iter().enumerate() {
            if taken[at] {
                continue;
            }
            let same = get(tx, id, "kind")? == json!(node.kind)
                && get(tx, id, "parent")? == parent
                && get(tx, id, "conjunction")? == node.conjunction
                && get(tx, id, "field")? == node.field
                && get(tx, id, "operator")? == node.operator
                && crate::json::equal(&get(tx, id, "value")?, &node.value);
            if same {
                taken[at] = true;
                chosen[index] = Some(id.clone());
                break;
            }
        }
    }
    let mut order: Vec<ObjectId> = Vec::with_capacity(wanted.len());
    for (index, node) in wanted.iter().enumerate() {
        let parent = node.parent.and_then(|parent| order.get(parent).cloned()).map_or(Value::Null, |id| json!(id.as_str()));
        let registers = [
            ("key", node.key.clone()),
            ("parent", parent),
            ("conjunction", node.conjunction.clone()),
            ("field", node.field.clone()),
            ("operator", node.operator.clone()),
            ("value", node.value.clone()),
        ];
        let id = match chosen[index].clone() {
            Some(id) => {
                for (name, value) in registers {
                    if !crate::json::equal(&tx.get(&id, name)?, &value) {
                        tx.set(&id, name, value)?;
                    }
                }
                id
            }
            None => {
                let mut values = Map::new();
                values.insert("kind".into(), json!(node.kind));
                for (name, value) in registers {
                    if !value.is_null() {
                        values.insert(name.into(), value);
                    }
                }
                let id = tx.create_with_id("Filter", &crate::ids::object(), values)?;
                let len = tx.list_items(view, "filters")?.len();
                tx.list_insert(view, "filters", len, &id)?;
                id
            }
        };
        order.push(id);
    }
    drop_unwanted(tx, view, "filters", &order)?;
    let raw = tx.list_items(view, "filters")?;
    place_items(tx, view, "filters", raw, &order, None)
}

/// The live items of a list, once each, in order.
fn live(replica: &Replica, id: &ObjectId, field: &str) -> Vec<ObjectId> {
    let mut seen = std::collections::HashSet::new();
    replica.list(id, field).unwrap_or_default().into_iter().filter(|item| replica.exists(item) && seen.insert(item.clone())).collect()
}

/// `view`'s settings in the document's form: what [`store`] wrote (the `config` register with
/// the collections put back under their keys). `null` for a view with neither.
pub fn assemble(replica: &Replica, view: &ObjectId) -> Result<Value> {
    let register = replica.get(view, "config")?;
    let mut out = register.as_object().cloned().unwrap_or_default();
    let mut any = register.is_object();
    for (key, field, _) in LISTS {
        let items: Vec<Value> = live(replica, view, field)
            .iter()
            .map(|id| {
                let mut item = Map::new();
                let key = replica.get(id, "key").unwrap_or(Value::Null);
                if !key.is_null() {
                    item.insert("id".into(), key);
                }
                item.insert("field".into(), replica.get(id, "field").unwrap_or(Value::Null));
                item.insert(
                    "direction".into(),
                    json!(replica.get(id, "direction").ok().and_then(|value| value.as_str().map(str::to_owned)).unwrap_or_else(|| "asc".into())),
                );
                Value::Object(item)
            })
            .collect();
        if !items.is_empty() {
            out.insert(key.into(), Value::Array(items));
            any = true;
        }
    }
    if let Some(tree) = filter_tree(replica, view) {
        out.insert(FILTER_TREE.into(), tree);
        any = true;
    }
    let mut hidden: Vec<(i64, String)> =
        replica.map_entries(view, HIDDEN.1)?.into_iter().map(|(id, place)| (place.as_i64().unwrap_or(i64::MAX), id)).collect();
    hidden.sort();
    if !hidden.is_empty() {
        out.insert(HIDDEN.0.into(), Value::Array(hidden.into_iter().map(|(_, id)| json!(id)).collect()));
        any = true;
    }
    let layout: Map<String, Value> = replica.map_entries(view, LAYOUT.1)?.into_iter().collect();
    if !layout.is_empty() {
        out.insert(LAYOUT.0.into(), Value::Object(layout));
        any = true;
    }
    Ok(if any { Value::Object(out) } else { Value::Null })
}

/// The filter tree `View.filters` holds: one root group (the first; the children of any other
/// root, which two people starting a filter at once make, are its children too).
fn filter_tree(replica: &Replica, view: &ObjectId) -> Option<Value> {
    let items = live(replica, view, "filters");
    if items.is_empty() {
        return None;
    }
    let get = |id: &ObjectId, name: &str| replica.get(id, name).unwrap_or(Value::Null);
    let is_group = |id: &ObjectId| get(id, "kind") == json!("group");
    let listed: std::collections::HashSet<&ObjectId> = items.iter().collect();
    // A node's group: its parent when that is a listed group, else the root.
    let parent_of = |id: &ObjectId| -> Option<ObjectId> {
        get(id, "parent").as_str().map(ObjectId::from_string).filter(|parent| listed.contains(parent) && is_group(parent) && parent != id)
    };
    let roots: Vec<&ObjectId> = items.iter().filter(|id| is_group(id) && parent_of(id).is_none()).collect();
    let root = roots.first().map(|id| (*id).clone());
    let mut children: HashMap<Option<ObjectId>, Vec<ObjectId>> = HashMap::new();
    for id in &items {
        if roots.contains(&id) {
            continue;
        }
        let parent = parent_of(id).filter(|parent| !roots.contains(&parent));
        children.entry(parent).or_default().push(id.clone());
    }
    fn node(
        id: Option<&ObjectId>,
        get: &dyn Fn(&ObjectId, &str) -> Value,
        children: &HashMap<Option<ObjectId>, Vec<ObjectId>>,
        depth: usize,
    ) -> Value {
        // The root's children are keyed by no parent (those of every root are its children).
        let kids_key = if depth == 0 { None } else { id.cloned() };
        let key = |id: Option<&ObjectId>| -> Value {
            match id {
                Some(id) => {
                    let key = get(id, "key");
                    if key.is_null() { json!(id.as_str()) } else { key }
                }
                None => json!("root"),
            }
        };
        if let Some(id) = id
            && get(id, "kind") == json!("condition")
        {
            return json!({ "id": key(Some(id)), "kind": "condition", "field": get(id, "field"), "operator": get(id, "operator"), "value": get(id, "value") });
        }
        let conjunction = id.map(|id| get(id, "conjunction")).filter(|value| !value.is_null()).unwrap_or_else(|| json!("and"));
        // A cycle a merge can make cannot run away: a node deeper than the list is long is left out.
        let kids: Vec<Value> = if depth > 64 {
            Vec::new()
        } else {
            children.get(&kids_key).into_iter().flatten().map(|child| node(Some(child), get, children, depth + 1)).collect()
        };
        json!({ "id": key(id), "kind": "group", "conjunction": conjunction, "children": kids })
    }
    Some(node(root.as_ref(), &get, &children, 0))
}
