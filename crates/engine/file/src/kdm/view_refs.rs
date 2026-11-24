//! Field references inside a view's config.
//!
//! Writers (the editor, the API) name a view config's fields by name (`field_name` in the entries
//! of `filter_tree`, `sorts` and `groups`; `cover_field_name`, `hidden_field_names`), and the
//! API's sorts and groups also by the field's integer id (`field_id`), which is local to one
//! file's projection. The document names them by object id instead (`field`, `cover_field`,
//! `hidden_fields`), so the references
//! survive a rename and mean the same field in every replica; [`config_to_kdm`] and
//! [`config_from_kdm`] convert between the two forms. A name that names no field of the view's
//! table is stored as `null` (a list of names leaves it out).
//!
//! `search_query` is the editor's search text: private to one person's session, it is never
//! stored in the document ([`PRIVATE_KEYS`]).

use kition_kdm::ObjectId;
use serde_json::{Map, Value, json};

/// Config keys whose value is a list of entries naming a field.
const ENTRY_LISTS: [&str; 2] = ["sorts", "groups"];
/// Entry lists whose entries the projection also gives the field's integer id, as the API wrote them.
const WITH_LOCAL_IDS: [&str; 2] = ["sorts", "groups"];
/// `(relational, document)` keys naming one field.
const FIELD_KEYS: [(&str, &str); 1] = [("cover_field_name", "cover_field")];

/// Config keys a view no longer has, each a second form of a setting it keeps: a single sort (one
/// of `sorts`), a single filter (a condition of `filter_tree`) and a group field (the first of
/// `groups`, also a board's columns). A config naming one is refused ([`refused_key`]).
pub const REMOVED_KEYS: [(&str, &str); 4] = [("sort", "sorts"), ("filter", "filter_tree"), ("group_field_name", "groups"), ("group_field", "groups")];

/// The first key of `config` a view no longer has, with the one that holds its setting.
pub fn refused_key(config: &Value) -> Option<(&'static str, &'static str)> {
    let map = config.as_object()?;
    REMOVED_KEYS.into_iter().find(|(key, _)| map.contains_key(*key))
}
/// `(relational, document)` keys naming a list of fields.
const FIELD_LISTS: [(&str, &str); 1] = [("hidden_field_names", "hidden_fields")];
/// The filter tree's key.
const FILTER_TREE: &str = "filter_tree";

/// Keys of a view config that are one person's state: never stored in the document.
pub const PRIVATE_KEYS: [&str; 1] = ["search_query"];

/// The document's form of a view config written by name: `field_of_name` answers the object id
/// of the view's table's field of a name. `field_id`s are dropped (name them first: an entry with
/// only a `field_id` is stored with `field: null`), and so are the [`PRIVATE_KEYS`].
pub fn config_to_kdm(config: &Value, field_of_name: &dyn Fn(&str) -> Option<ObjectId>) -> Value {
    let Value::Object(map) = config else {
        return config.clone();
    };
    let mut out = map.clone();
    for key in PRIVATE_KEYS {
        out.remove(key);
    }
    for key in ENTRY_LISTS {
        if let Some(Value::Array(items)) = out.get_mut(key) {
            for item in items {
                entry_to_kdm(item, field_of_name);
            }
        }
    }
    if let Some(tree) = out.get_mut(FILTER_TREE) {
        tree_to_kdm(tree, field_of_name);
    }
    for (relational, kdm) in FIELD_KEYS {
        if let Some(name) = out.remove(relational) {
            let id = name.as_str().and_then(field_of_name);
            out.insert(kdm.to_owned(), id.map_or(Value::Null, |id| json!(id.as_str())));
        }
    }
    for (relational, kdm) in FIELD_LISTS {
        if let Some(names) = out.remove(relational) {
            let ids: Vec<Value> =
                names.as_array().into_iter().flatten().filter_map(Value::as_str).filter_map(field_of_name).map(|id| json!(id.as_str())).collect();
            out.insert(kdm.to_owned(), Value::Array(ids));
        }
    }
    Value::Object(out)
}

fn entry_to_kdm(item: &mut Value, field_of_name: &dyn Fn(&str) -> Option<ObjectId>) {
    let Value::Object(entry) = item else {
        return;
    };
    let name = entry.remove("field_name");
    let local = entry.remove("field_id");
    if name.is_none() && local.is_none() {
        return;
    }
    let id = name.as_ref().and_then(Value::as_str).and_then(field_of_name);
    entry.insert("field".into(), id.map_or(Value::Null, |id| json!(id.as_str())));
}

fn tree_to_kdm(node: &mut Value, field_of_name: &dyn Fn(&str) -> Option<ObjectId>) {
    if node.get("kind").and_then(Value::as_str) == Some("condition") {
        entry_to_kdm(node, field_of_name);
    }
    if let Some(Value::Array(children)) = node.get_mut("children") {
        for child in children {
            tree_to_kdm(child, field_of_name);
        }
    }
}

/// What the projection names a field by: its name, and its integer id when it has a row.
pub type Described = Option<(String, Option<i64>)>;

/// The relational form of a document view config: every field reference by name
/// ([`config_to_kdm`] in reverse), with the integer id in the entries of `sorts` and `groups`.
/// `describe` answers a field object's name and row id; a reference it does not know (an object
/// that never had a row) is left out.
pub fn config_from_kdm(config: &Value, describe: &dyn Fn(&ObjectId) -> Described) -> Value {
    let Value::Object(map) = config else {
        return config.clone();
    };
    let mut out = map.clone();
    for key in ENTRY_LISTS {
        let local = WITH_LOCAL_IDS.contains(&key);
        if let Some(Value::Array(items)) = out.get_mut(key) {
            for item in items {
                entry_from_kdm(item, describe, local);
            }
        }
    }
    if let Some(tree) = out.get_mut(FILTER_TREE) {
        tree_from_kdm(tree, describe);
    }
    for (relational, kdm) in FIELD_KEYS {
        if let Some(id) = out.remove(kdm)
            && let Some((name, _)) = id.as_str().and_then(|id| describe(&ObjectId::from_string(id)))
        {
            out.insert(relational.to_owned(), json!(name));
        }
    }
    for (relational, kdm) in FIELD_LISTS {
        if let Some(ids) = out.remove(kdm) {
            let names: Vec<Value> = ids
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(Value::as_str)
                .filter_map(|id| describe(&ObjectId::from_string(id)))
                .map(|(name, _)| json!(name))
                .collect();
            out.insert(relational.to_owned(), Value::Array(names));
        }
    }
    Value::Object(out)
}

fn entry_from_kdm(item: &mut Value, describe: &dyn Fn(&ObjectId) -> Described, local: bool) {
    let Value::Object(entry) = item else {
        return;
    };
    let Some(field) = entry.remove("field") else {
        return;
    };
    let mut named = Map::new();
    if let Some((name, id)) = field.as_str().and_then(|id| describe(&ObjectId::from_string(id))) {
        named.insert("field_name".into(), json!(name));
        if local && let Some(id) = id {
            named.insert("field_id".into(), json!(id));
        }
    }
    // The name first, as writers put it.
    named.extend(std::mem::take(entry));
    *entry = named;
}

fn tree_from_kdm(node: &mut Value, describe: &dyn Fn(&ObjectId) -> Described) {
    if node.get("kind").and_then(Value::as_str) == Some("condition") {
        entry_from_kdm(node, describe, false);
    }
    if let Some(Value::Array(children)) = node.get_mut("children") {
        for child in children {
            tree_from_kdm(child, describe);
        }
    }
}

/// A relational view config with every entry that names its field only by integer id
/// (`field_id`, as the API's sorts and groups do) named by `name_of_local` too, so
/// [`config_to_kdm`] can resolve it.
pub fn name_local_ids(config: &Value, name_of_local: &dyn Fn(i64) -> Option<String>) -> Value {
    let Value::Object(map) = config else {
        return config.clone();
    };
    let mut out = map.clone();
    let name = |item: &mut Value| {
        if let Value::Object(entry) = item
            && !entry.contains_key("field_name")
            && let Some(name) = entry.get("field_id").and_then(Value::as_i64).and_then(name_of_local)
        {
            entry.insert("field_name".into(), json!(name));
        }
    };
    for key in ENTRY_LISTS {
        if let Some(Value::Array(items)) = out.get_mut(key) {
            for item in items {
                name(item);
            }
        }
    }
    fn tree(node: &mut Value, name: &dyn Fn(&mut Value)) {
        if node.get("kind").and_then(Value::as_str) == Some("condition") {
            name(node);
        }
        if let Some(Value::Array(children)) = node.get_mut("children") {
            for child in children {
                tree(child, name);
            }
        }
    }
    if let Some(node) = out.get_mut(FILTER_TREE) {
        tree(node, &name);
    }
    Value::Object(out)
}
