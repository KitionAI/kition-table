//! An explicit `name` is kept as given, or refused.
//!
//! A table's or field's `name` is its internal name: lowercase ASCII letters, digits and `_`,
//! starting with a letter and not ending with `_` ([`internal_name`] of itself), unique among the
//! base's tables or the table's fields. Without one, the name is derived from the title the same
//! way and suffixed `_2`, `_3`, ... when taken. A host that names a field writes its values under
//! that name, so an explicit name the engine would derive differently (one that is not in that
//! form, or one already taken, also by an earlier field of the same request) is refused before
//! anything is written, one [`ParamIssue`] each: `tables.create` (`/name`, `/fields/<i>/name`)
//! and `fields.create` (`/name`).

use std::collections::HashSet;

use kition_table_file::{TableFile, internal_name};
use serde_json::{Map, Value};

use crate::{ApiError, ApiResult, ParamIssue};

/// What a name may be, for the issues' `expected`.
pub const NAME_RULE: &str =
    "lowercase ASCII letters, digits and `_`, starting with a letter and not ending with `_` (`^[a-z](?:[a-z0-9_]*[a-z0-9])?$`), unique in its table";

/// Refuses `method`'s request when an explicit name would not be kept as given
/// ([`ApiError::InvalidParams`], one issue per name).
pub(crate) fn refuse_rewritten_names(file: &TableFile, method: &str, params: &Map<String, Value>) -> ApiResult<()> {
    let mut issues = Vec::new();
    match method {
        "tables.create" => {
            let taken: HashSet<String> = file.list_tables().map(|tables| tables.into_iter().map(|table| table.name).collect()).unwrap_or_default();
            let title = text(params, "title");
            name_kept(params, &title, "Table", &taken, "/name", "name", "a table of this document", &mut issues);
            let fields = params.get("fields").and_then(Value::as_array).map(Vec::as_slice).unwrap_or_default();
            let mut taken = HashSet::new();
            for (i, field) in fields.iter().enumerate() {
                let Value::Object(field) = field else { continue };
                let title = text(field, "title");
                let kept = name_kept(
                    field,
                    &title,
                    "Field",
                    &taken,
                    &format!("/fields/{i}/name"),
                    &format!("fields[{i}].name"),
                    "another field of this request",
                    &mut issues,
                );
                taken.insert(kept);
            }
        }
        "fields.create" => {
            let table = params.get("tableId").and_then(|id| id.as_i64().or_else(|| id.as_str()?.parse().ok()));
            if let Some(Ok(fields)) = table.map(|table| file.list_fields(table)) {
                let taken: HashSet<String> = fields.into_iter().map(|field| field.name).collect();
                let title = text(params, "title");
                name_kept(params, &title, "Field", &taken, "/name", "name", "a field of this table", &mut issues);
            }
        }
        _ => {}
    }
    if issues.is_empty() { Ok(()) } else { Err(ApiError::InvalidParams(issues)) }
}

fn text(object: &Map<String, Value>, key: &str) -> String {
    object.get(key).and_then(Value::as_str).unwrap_or_default().trim().to_owned()
}

/// The name the engine gives an object with `object`'s `name` and `title` (`default` without
/// one) when `taken` are taken: an issue when an explicit name is not that name.
#[allow(clippy::too_many_arguments)]
fn name_kept(
    object: &Map<String, Value>,
    title: &str,
    default: &str,
    taken: &HashSet<String>,
    pointer: &str,
    shown: &str,
    whose: &str,
    issues: &mut Vec<ParamIssue>,
) -> String {
    let given = object.get("name").and_then(Value::as_str).unwrap_or_default();
    let title = if title.is_empty() { default } else { title };
    let base = internal_name(if given.trim().is_empty() { title } else { given.trim() });
    let mut kept = base.clone();
    let mut suffix = 2;
    while taken.contains(&kept) {
        kept = format!("{base}_{suffix}");
        suffix += 1;
    }
    if !given.is_empty() && kept != given {
        let message = if base != given {
            format!("`{shown}` \"{given}\" is not an internal name; as one it would be `{base}`")
        } else {
            format!("`{shown}` \"{given}\" is taken by {whose}")
        };
        issues.push(ParamIssue { path: pointer.to_owned(), message, expected: NAME_RULE.to_owned() });
    }
    kept
}

#[cfg(test)]
mod tests {
    use super::*;

    fn kept(name: &str, title: &str, taken: &[&str]) -> (String, Vec<ParamIssue>) {
        let mut issues = Vec::new();
        let object = serde_json::json!({ "name": name }).as_object().cloned().unwrap_or_default();
        let taken = taken.iter().map(|name| (*name).to_owned()).collect();
        (name_kept(&object, title, "Field", &taken, "/name", "name", "a field of this table", &mut issues), issues)
    }

    #[test]
    fn keeps_a_name_in_form_and_refuses_one_the_engine_would_change() {
        assert_eq!(kept("email", "Email", &[]), ("email".to_owned(), Vec::new()));
        assert!(kept("", "电话", &[]).1.is_empty(), "a derived name is never refused");
        assert_eq!(kept("", "电话", &["field"]).0, "field_2");
        for (name, message) in [
            ("电话x", "`name` \"电话x\" is not an internal name; as one it would be `x`"),
            ("Email", "`name` \"Email\" is not an internal name; as one it would be `email`"),
            ("email_", "`name` \"email_\" is not an internal name; as one it would be `email`"),
            (" email", "`name` \" email\" is not an internal name; as one it would be `email`"),
        ] {
            let (_, issues) = kept(name, "Email", &[]);
            assert_eq!(issues.iter().map(|issue| issue.message.as_str()).collect::<Vec<_>>(), [message], "{name}");
        }
        let (kept_as, issues) = kept("name", "Dup", &["name"]);
        assert_eq!(kept_as, "name_2");
        assert_eq!(issues[0].message, "`name` \"name\" is taken by a field of this table");
        assert_eq!(issues[0].expected, NAME_RULE);
    }
}
