//! Per-call premises, in the replica's persistent id space. The relational API's integers
//! are local projection addresses, so resolving them here is essential even with objects
//! declared on the command. Tombstone rows deliberately participate in that resolution.

use std::collections::BTreeMap;

use kition_kdm::ObjectId;
use kition_kit::{KitCommand, Premise, Under, Within};
use kition_table_file::TableFile;
use serde_json::{Value, json};

use crate::params;

pub fn resolve(file: &TableFile, ty: &str, value: &Value) -> Option<String> {
    let (table, column) = match ty {
        "Table" => ("kition_tables", "kdm_id"),
        "Field" => ("kition_fields", "kdm_id"),
        "View" => ("kition_views", "kdm_id"),
        "Record" => ("kition_records", "row_key"),
        "Dashboard" | "Link" => return value.as_str().map(str::to_owned),
        _ => return None,
    };
    if let Some(id) = value.as_i64().or_else(|| value.as_str()?.parse().ok()) {
        file.conn().query_row(&format!("SELECT {column} FROM {table} WHERE id = ?1"), [id], |r| r.get(0)).ok()
    } else {
        value.as_str().map(str::to_owned)
    }
}

/// The premise contract of a raw cell write, `map.set` or `map.remove` of a record's `cells`
/// (`batchOps` in `model/schema.json`; the write itself runs as the raw operation): the one cell
/// it assigns, and the field its key names: that it is still there, and its `type` and `options`,
/// through which the written value is read. Neither the record's place nor its other cells, nor
/// the field's name, title or place.
pub const RAW_CELL: &str = "model.cell";

pub fn raw_cell(file: &TableFile, params: &Value) -> Result<Under, String> {
    let record = params["target"].as_str().ok_or("a raw cell write names its record by id")?;
    let key = params["key"].as_str().ok_or("a raw cell write names its field by its id, as the map key")?;
    let mut premises = vec![Premise::on(json!(record)).fields([format!("cells.{key}")]).positions(Vec::<String>::new())];
    if file.replica().object_type(&ObjectId::from_string(key)).is_ok_and(|ty| ty == "Field") {
        premises.push(Premise::on(json!(key)).fields(["options", "type"].map(str::to_owned)).positions(Vec::<String>::new()));
    }
    Ok(Under { premises, ..Under::default() })
}

#[derive(Default)]
pub(crate) struct Needs(BTreeMap<String, Premise>);

impl Needs {
    pub(crate) fn full(&mut self, id: String) {
        self.0.insert(id.clone(), Premise::on(json!(id)));
    }

    pub(crate) fn set(&mut self, id: String, fields: Vec<String>, positions: Vec<String>) {
        self.0.insert(id.clone(), Premise::on(json!(id)).fields(fields).positions(positions));
    }

    pub(crate) fn add(&mut self, id: String, fields: Vec<String>) {
        let p = self.0.entry(id.clone()).or_insert_with(|| Premise::on(json!(id)).fields(Vec::<String>::new()).positions(Vec::<String>::new()));
        if let Some(known) = &mut p.fields {
            known.extend(fields);
            known.sort();
            known.dedup();
        }
    }

    /// Only that the object is there: narrows a whole premise (an object parameter's default)
    /// and keeps what an earlier premise listed.
    pub(crate) fn exists(&mut self, id: String) {
        let premise = self.0.entry(id.clone()).or_insert_with(|| Premise::on(json!(id)).fields(Vec::<String>::new()).positions(Vec::<String>::new()));
        if premise.fields.is_none() {
            premise.fields = Some(Vec::new());
        }
        if premise.positions.is_none() {
            premise.positions = Some(Vec::new());
        }
    }

    /// Every field but no place: what a call deleting or replacing an object whole rests on.
    fn wherever(&mut self) {
        for premise in self.0.values_mut() {
            if premise.fields.is_none() && premise.positions.is_none() {
                premise.positions = Some(Vec::new());
            }
        }
    }

    pub(crate) fn select_labels(&mut self, id: &str) {
        if let Some(fields) = self.0.get_mut(id).and_then(|p| p.fields.as_mut()) {
            fields.retain(|field| field != "options");
        }
    }

    fn position(&mut self, id: String, field: &str) {
        self.add(id.clone(), Vec::new());
        if let Some(known) = self.0.get_mut(&id).and_then(|p| p.positions.as_mut()) {
            known.push(field.into());
            known.sort();
            known.dedup();
        }
    }
}

const SETTINGS: &[(&str, &str)] = &[
    ("row_height", "rowHeight"),
    ("frozen_column_count", "frozenColumnCount"),
    ("search_query", "searchQuery"),
    ("filter_tree", "filterTree"),
    ("sorts", "sorts"),
    ("groups", "groups"),
];

pub fn derived(file: &TableFile, ids: &[Value], names: &[String]) -> Result<Vec<kition_kit::DerivedValues>, String> {
    let mut result = file
        .replica()
        .objects_of_type("View")
        .into_iter()
        .filter(|id| ids.is_empty() || ids.iter().any(|v| v.as_str() == Some(id.as_str())))
        .map(|id| {
            // An absent config and an object without the key have the same effective value.
            let config = file.replica().get(&id, "config").map_err(|e| e.to_string())?;
            let values = SETTINGS
                .iter()
                .filter(|(_, name)| names.is_empty() || names.iter().any(|n| n == name))
                .map(|(key, name)| ((*name).to_owned(), config.get(key).cloned().unwrap_or(Value::Null)))
                .chain(
                    (names.is_empty() || names.iter().any(|n| n == "owner"))
                        .then(|| ("owner".to_owned(), crate::dependencies::owner(file, id.as_str()))),
                )
                .collect();
            Ok(kition_kit::DerivedValues { id: json!(id.as_str()), values })
        })
        .collect::<Result<Vec<_>, String>>()?;
    if names.is_empty() || names.iter().any(|n| n == "owner") {
        for ty in ["Field", "Record"] {
            for id in
                file.replica().objects_of_type(ty).into_iter().filter(|id| ids.is_empty() || ids.iter().any(|v| v.as_str() == Some(id.as_str())))
            {
                result.push(kition_kit::DerivedValues {
                    id: json!(id.as_str()),
                    values: serde_json::Map::from_iter([("owner".to_owned(), crate::dependencies::owner(file, id.as_str()))]),
                });
            }
        }
    }
    result.extend(crate::dependencies::derived(file, ids, names));
    Ok(result)
}

fn strings(items: &[&str]) -> Vec<String> {
    items.iter().map(|s| (*s).to_owned()).collect()
}

fn patches(params: &Value, keys: &[(&str, &str)]) -> Vec<String> {
    keys.iter().filter(|(key, _)| params.get(*key).is_some()).map(|(_, field)| (*field).to_owned()).collect()
}

fn gap(file: &TableFile, parent: &str, field: &str, at: usize) -> Within {
    let len = file.replica().list(&ObjectId::from_string(parent), field).map_or(0, |v| v.len());
    let at = at.min(len);
    Within { parent: json!(parent), field: field.into(), from: at, to: at, ..Default::default() }
}

fn cells(file: &TableFile, needs: &mut Needs, table: i64, record: Option<String>, values: &Value) {
    let Ok(table) = file.get_table(table) else { return };
    let mut held = Vec::new();
    for field in &table.fields {
        let written = values.get(&field.name).is_some();
        if !written {
            continue;
        }
        let Some(id) = resolve(file, "Field", &json!(field.id)) else { continue };
        // A value is written by its field's type (a computed field's is dropped), a link's or a
        // select's by its options too. The required check only refuses the call, and no write
        // reads `readonly` or a formula, so neither is held.
        let link_or_select = matches!(field.field_type.as_str(), "link_to_record" | "single_select" | "multi_select");
        needs.add(id.clone(), strings(if link_or_select { &["name", "type", "options"] } else { &["name", "type"] }));
        held.push(format!("cells.{id}"));
        if field.field_type == "link_to_record" {
            for value in values[&field.name].as_array().into_iter().flatten() {
                let value = value.get("row_key").unwrap_or(value);
                if let Some(id) = resolve(file, "Record", value) {
                    needs.add(id, Vec::new());
                }
            }
        }
    }
    if let Some(record) = record {
        needs.add(record, held);
    }
}

pub fn under(file: &TableFile, command: &KitCommand, params: &Value) -> Result<Option<Under>, String> {
    let local_windows = crate::address::windows(file, command, params);
    let normalized = crate::address::normalize(file, command, params)?;
    let params = &normalized;
    let method = command.id.strip_prefix("table.").unwrap_or(&command.id);
    let mut needs = Needs::default();
    for (path, types) in command.object_types {
        if command.name_params.iter().any(|(p, _)| p == path) {
            continue;
        }
        for value in params::values(params, path) {
            for ty in *types {
                if let Some(id) = resolve(file, ty, value) {
                    needs.full(id.clone());
                    if *ty == "Table" && method != "tables.delete" {
                        needs.set(id, Vec::new(), Vec::new());
                    }
                }
            }
        }
    }
    let named = |key: &str, ty: &str| params.get(key).and_then(|v| resolve(file, ty, v));
    let table = named("tableId", "Table");
    let field = named("fieldId", "Field");
    let record = named("recordId", "Record");
    let view = named("viewId", "View");
    let mut within = local_windows;
    // Table ids in child commands are ownership/existence premises. Holding the parent whole
    // would turn a colleague's edit to any other record into a false refusal.
    if let Some(table) = &table
        && !matches!(method, "tables.update" | "tables.delete" | "import.run" | "csv.import")
    {
        needs.set(table.clone(), Vec::new(), Vec::new());
    }
    match method {
        "tables.update" => {
            if let Some(table) = table {
                needs.set(
                    table,
                    patches(params, &[("title", "title"), ("description", "description"), ("meta", "meta"), ("primary_field_id", "primaryField")]),
                    Vec::new(),
                );
            }
            // The new primary field: that it is one of the table's fields (`~owner`), nothing of it.
            if let Some(field) = named("primary_field_id", "Field") {
                needs.exists(field);
            }
        }
        "fields.update" => {
            if let Some(field) = field {
                needs.set(
                    field,
                    patches(
                        params,
                        &[
                            ("title", "title"),
                            ("type", "type"),
                            ("required", "required"),
                            ("readonly", "readonly"),
                            ("options", "options"),
                            ("formula", "formula"),
                            ("ai_prompt", "aiPrompt"),
                            ("meta", "meta"),
                        ],
                    ),
                    Vec::new(),
                );
                if params.get("is_primary").is_some()
                    && let Some(table) = table
                {
                    needs.add(table, strings(&["primaryField"]));
                }
            }
        }
        "views.update" => {
            if let Some(view) = view {
                needs.set(view, patches(params, &[("title", "title"), ("type", "type"), ("config", "config")]), Vec::new());
            }
        }
        "views.patchConfig" => {
            if let Some(view) = view {
                let fields = SETTINGS.iter().take(3).filter(|(key, _)| params.get(*key).is_some()).map(|(_, name)| format!("~{name}")).collect();
                needs.set(view, fields, Vec::new());
            }
        }
        "views.putFilterTree" | "views.putSorts" | "views.putGroups" => {
            if let Some(view) = view {
                let key = match method {
                    "views.putFilterTree" => "filterTree",
                    "views.putSorts" => "sorts",
                    _ => "groups",
                };
                needs.set(view, vec![format!("~{key}")], Vec::new());
            }
        }
        "views.patchField" => {
            if let (Some(view), Some(field)) = (view, field) {
                let mut held = Vec::new();
                if params.get("width").is_some() {
                    held.push(format!("widths.{field}"));
                }
                if params.get("visible").is_some() {
                    held.push(format!("hidden.{field}"));
                }
                needs.set(view, held, Vec::new());
                needs.set(field, Vec::new(), Vec::new());
            }
        }
        "records.update" | "records.create" => {
            if let Some(id) = &record {
                needs.set(id.clone(), Vec::new(), Vec::new());
            }
            if let Some(t) = params["tableId"].as_i64() {
                cells(file, &mut needs, t, record.clone(), &params["values"]);
            }
            if method == "records.update"
                && let Some(order) = params["order"].as_i64().filter(|v| *v > 0)
                && let Some(table) = table
            {
                let records = file.list_records(params["tableId"].as_i64().unwrap_or_default()).map_err(|e| e.to_string())?;
                let at = records.iter().position(|r| Some(r.row_key.as_str()) != record.as_deref() && r.order >= order).unwrap_or(records.len());
                within.push(gap(file, &table, "records", at));
                if let Some(record) = record {
                    needs.position(record, "records");
                }
            }
        }
        "fields.create" => {
            if let (Some(table), Some(order)) = (table, params["order"].as_i64()) {
                within.push(gap(file, &table, "fields", usize::try_from(order.max(1) - 1).unwrap_or(usize::MAX)));
            }
        }
        "records.bulkCopy" => {
            for value in params::values(params, "row_keys") {
                if let Some(id) = resolve(file, "Record", value) {
                    needs.set(id, strings(&["cells"]), Vec::new());
                }
            }
            if let (Some(table), Some(position)) = (table, params["target_position"].as_i64().filter(|v| *v > 0)) {
                let records = file.list_records(params["tableId"].as_i64().unwrap_or_default()).map_err(|e| e.to_string())?;
                let at = records.iter().position(|r| r.order >= position).unwrap_or(records.len());
                within.push(gap(file, &table, "records", at));
            }
        }
        "transact" => {
            needs = Needs::default();
            for op in params["ops"].as_array().into_iter().flatten() {
                let Some(table) = op.get("tableId").and_then(|v| resolve(file, "Table", v)) else { continue };
                needs.add(table.clone(), Vec::new());
                match op["op"].as_str() {
                    Some("cells.set") => {
                        for row in op["records"].as_array().into_iter().flatten() {
                            let record = row.get("rowKey").and_then(|v| resolve(file, "Record", v));
                            cells(file, &mut needs, op["tableId"].as_i64().unwrap_or_default(), record, &row["values"]);
                        }
                    }
                    Some("records.delete") => {
                        for id in params::values(op, "rowKeys").into_iter().filter_map(|v| resolve(file, "Record", v)) {
                            needs.full(id);
                        }
                    }
                    Some("records.create") => {
                        for values in op["records"].as_array().into_iter().flatten() {
                            cells(file, &mut needs, op["tableId"].as_i64().unwrap_or_default(), None, values);
                        }
                        for key in ["before", "after"] {
                            if let Some(id) = op["at"].get(key).and_then(|v| resolve(file, "Record", v)) {
                                needs.position(id, "records");
                            }
                        }
                    }
                    Some("fields.move") => {
                        let moving: Vec<_> = params::values(op, "fieldIds").into_iter().filter_map(|v| resolve(file, "Field", v)).collect();
                        let listing = file.replica().list(&ObjectId::from_string(&table), "fields").map_err(|e| e.to_string())?;
                        let index = op["toIndex"].as_u64().unwrap_or_default() as usize;
                        let at = listing
                            .iter()
                            .enumerate()
                            .filter(|(_, id)| !moving.iter().any(|v| v == id.as_str()))
                            .nth(index)
                            .map_or(listing.len(), |(i, _)| i);
                        within.push(gap(file, &table, "fields", at));
                        for id in moving {
                            needs.position(id, "fields");
                        }
                    }
                    Some("records.move") => {
                        for id in params::values(op, "rowKeys").into_iter().filter_map(|v| resolve(file, "Record", v)) {
                            needs.position(id, "records");
                        }
                        if let Some(id) = op.get("before").and_then(|v| resolve(file, "Record", v)) {
                            needs.position(id, "records");
                        }
                    }
                    _ => {}
                }
            }
        }
        _ => {}
    }
    let mut selected = crate::dependencies::refine(file, method, params, &mut needs);
    // A delete or a whole replacement rests on every field of what it removes, not on where it is.
    if matches!(
        method,
        "tables.delete"
            | "fields.delete"
            | "views.delete"
            | "records.delete"
            | "records.bulkDelete"
            | "dashboards.upsert"
            | "dashboards.delete"
            | "transact"
            | "import.run"
    ) {
        needs.wherever();
    }
    within.append(&mut selected.windows);
    selected.windows = within.clone();
    owners(file, command, params, &mut selected);
    for id in &selected.owners {
        needs.add(id.clone(), vec!["~owner".into()]);
    }
    let covers = !selected.is_empty();
    let mut objects: Vec<Value> = within
        .iter()
        .filter(|w| w.from != w.to)
        .filter_map(|w| file.replica().list(&ObjectId::from_string(w.parent.as_str()?), &w.field).ok()?.get(w.from).map(|id| json!(id.as_str())))
        .collect();
    if covers {
        // A table or field named by a name the call reads is held by that name's membership
        // (`covered`), not by its other fields.
        for id in selected.objects(file) {
            if let Some(text) = id.as_str()
                && file.replica().object_type(&ObjectId::from_string(text)).is_ok_and(|ty| ty != "Link")
                && !needs.0.contains_key(text)
            {
                needs.exists(text.to_owned());
            }
            objects.push(id);
        }
    }
    Ok(Some(Under {
        objects,
        covers: covers.then(|| kition_kit::Covers {
            fields: strings(&["name", "from", "field", "to", "config", "options", "~owner", "~fieldOrder", "~linkCells", "~mirrors", "~rowIds"]),
        }),
        premises: needs.0.into_values().collect(),
        within,
        ..Under::default()
    }))
}

pub fn covered(
    file: &TableFile,
    command: &KitCommand,
    params: &Value,
    objects: &[kition_kit::CoveredAsk],
) -> Result<Option<kition_kit::CoveredAt>, String> {
    let windows = under(file, command, params)?.map(|under| under.within).unwrap_or_default();
    let params = crate::address::normalize(file, command, params)?;
    let mut selected = crate::dependencies::refine(file, command.id.strip_prefix("table.").unwrap_or(&command.id), &params, &mut Needs::default());
    selected.windows = windows;
    owners(file, command, &params, &mut selected);
    crate::dependencies::covered(file, &selected, objects).map(Some)
}

/// What `covered` refuses for, as facts (`Selection::facts`), for the comparison with the handlers' reading.
pub fn facts(file: &TableFile, command: &KitCommand, params: &Value) -> Result<Vec<(String, String)>, String> {
    let params = crate::address::normalize(file, command, params)?;
    let selected = crate::dependencies::refine(file, command.id.strip_prefix("table.").unwrap_or(&command.id), &params, &mut Needs::default());
    Ok(selected.facts(file))
}

fn owners(file: &TableFile, command: &KitCommand, params: &Value, selected: &mut crate::dependencies::Selection) {
    for (path, types) in command.object_types {
        if command.name_params.iter().any(|(name, _)| name == path) {
            continue;
        }
        for value in params::values(params, path) {
            for ty in *types {
                if matches!(*ty, "Field" | "View" | "Record")
                    && let Some(id) = resolve(file, ty, value)
                {
                    selected.owners.insert(id);
                }
            }
        }
    }
}

pub fn places(command: &str) -> &'static [&'static str] {
    match command {
        "table.tables.create" => &["title", "name"],
        "table.import.run" => &["target.table_title"],
        "table.records.update" => &["order", "values"],
        "table.records.create" => &["values"],
        "table.fields.create" => &["order"],
        "table.records.bulkCopy" => &["target_position"],
        "table.views.patchField" => &["position", "frozen", "width", "visible"],
        "table.transact" => &["ops"],
        _ => &[],
    }
}
