//! Dependencies resolved from the same field definitions as the request handlers.

use std::collections::{BTreeMap, BTreeSet};

use kition_kdm::ObjectId;
use kition_kit::{Covered, CoveredAsk, CoveredAt};
use kition_table_file::{Field, TableFile};
use serde_json::{Value, json};

use crate::place::{Needs, resolve};

#[derive(Default)]
pub(crate) struct Selection {
    pub names: BTreeMap<i64, BTreeSet<String>>,
    pub generated_names: BTreeMap<i64, BTreeSet<String>>,
    pub table_names: BTreeSet<String>,
    pub links: BTreeSet<(String, String)>,
    pub mirrors: BTreeSet<String>,
    pub owners: BTreeSet<String>,
    pub windows: Vec<kition_kit::Within>,
    pub row_tables: BTreeSet<String>,
    pub schema_tables: BTreeSet<String>,
    pub record_links: BTreeSet<String>,
    pub field_links: BTreeSet<String>,
}

impl Selection {
    fn name(&mut self, table: i64, name: &str) {
        self.names.entry(table).or_default().insert(name.to_owned());
    }

    pub fn objects(&self, file: &TableFile) -> Vec<Value> {
        let mut objects = Vec::new();
        for table in file.list_tables().unwrap_or_default() {
            if self.table_names.contains(&table.name)
                && let Some(id) = resolve(file, "Table", &json!(table.id))
            {
                objects.push(json!(id));
            }
        }
        for (table, names) in self.names.iter().chain(&self.generated_names) {
            for field in file.list_fields(*table).unwrap_or_default() {
                if names.contains(&field.name)
                    && let Some(id) = resolve(file, "Field", &json!(field.id))
                {
                    objects.push(json!(id));
                }
            }
        }
        for id in file.replica().objects_of_type("Link") {
            let value = |key| file.replica().get(&id, key).unwrap_or(Value::Null);
            if let (Some(from), Some(field)) = (value("from").as_str(), value("field").as_str())
                && self.links.contains(&(from.to_owned(), field.to_owned()))
            {
                objects.push(json!(id.as_str()));
            }
        }
        objects
    }

    /// What the selection holds, as `(object id, fact)` pairs in the words of the independent
    /// reading's comparison (`tests/declarations_independent.rs`): name membership, link
    /// membership, partners, row identities and field lists.
    pub fn facts(&self, file: &TableFile) -> Vec<(String, String)> {
        let root = file.replica().objects_of_type("Base").first().map(|id| id.to_string()).unwrap_or_default();
        let table_id = |table: &i64| resolve(file, "Table", &json!(table)).unwrap_or_default();
        let mut facts = Vec::new();
        for (table, names) in self.names.iter().chain(&self.generated_names) {
            facts.extend(names.iter().map(|name| (table_id(table), format!("~names:{name}"))));
        }
        facts.extend(self.table_names.iter().map(|name| (root.clone(), format!("~tableNames:{name}"))));
        facts.extend(self.links.iter().map(|(record, field)| (root.clone(), format!("~linkCells:{record}.{field}"))));
        facts.extend(self.record_links.iter().map(|record| (root.clone(), format!("~linkCells:{record}"))));
        facts.extend(self.field_links.iter().map(|field| (root.clone(), format!("~linkCells:*.{field}"))));
        facts.extend(self.mirrors.iter().map(|field| (root.clone(), format!("~mirrors:{field}"))));
        facts.extend(self.row_tables.iter().map(|table| (root.clone(), format!("~rowIds:{table}"))));
        // The reading's word for a table's field list; the kit holds it as that table's
        // `~fieldOrder` (one small value per table).
        facts.extend(self.schema_tables.iter().map(|table| (root.clone(), format!("~lists:{table}.fields"))));
        facts
    }

    pub fn is_empty(&self) -> bool {
        self.names.is_empty()
            && self.generated_names.is_empty()
            && self.table_names.is_empty()
            && self.links.is_empty()
            && self.mirrors.is_empty()
            && self.owners.is_empty()
            && self.windows.is_empty()
            && self.row_tables.is_empty()
            && self.schema_tables.is_empty()
            && self.record_links.is_empty()
            && self.field_links.is_empty()
    }
}

/// What decides a field's value as a reader sees it: its name and type, a formula's formula, a
/// link's, lookup's or rollup's options.
fn definition(file: &TableFile, needs: &mut Needs, field: &Field) -> Option<String> {
    let id = resolve(file, "Field", &json!(field.id))?;
    let mut fields = vec!["name".to_owned(), "type".to_owned()];
    if field.field_type == "formula" {
        fields.push("formula".into());
    }
    if matches!(field.field_type.as_str(), "link_to_record" | "lookup" | "rollup") {
        fields.push("options".into());
    }
    needs.add(id.clone(), fields);
    Some(id)
}

/// The select values a call writes, by field: the labels, or `None` once one of them names no
/// choice the field lists.
type Labels = BTreeMap<String, Option<BTreeSet<String>>>;

fn select_labels(file: &TableFile, labels: &mut Labels, table: i64, values: &Value) {
    for field in file.list_fields(table).unwrap_or_default() {
        if !matches!(field.field_type.as_str(), "single_select" | "multi_select") {
            continue;
        }
        let Some(value) = values.get(&field.name) else { continue };
        let Some(id) = resolve(file, "Field", &json!(field.id)) else { continue };
        let written: Vec<String> =
            value.as_array().map(Vec::as_slice).unwrap_or(std::slice::from_ref(value)).iter().filter_map(Value::as_str).map(str::to_owned).collect();
        let choices = kition_table_file::kdm::cells::listed_choices(file.replica(), &ObjectId::from_string(&id)).unwrap_or_default();
        let entry = labels.entry(id).or_insert_with(|| Some(BTreeSet::new()));
        if !written.iter().all(|label| choices.iter().any(|choice| &choice.name == label)) {
            *entry = None;
        } else if let Some(known) = entry {
            known.extend(written);
        }
    }
}

/// Narrows the premises of the selects a call writes. A label missing now may have left the
/// choices since basis (renamed, or no longer listed): such a field keeps the whole premise on its
/// choices and their names, so that edit refuses as `changed`. Labels the field lists hold only
/// those choices' names; sibling choices, their order and new ones stay independent.
fn narrow_select_labels(file: &TableFile, needs: &mut Needs, labels: &Labels) {
    for (id, written) in labels {
        let Some(written) = written else { continue };
        let field = ObjectId::from_string(id);
        let choices = kition_table_file::kdm::cells::listed_choices(file.replica(), &field).unwrap_or_default();
        let named: Vec<String> = choices.iter().filter(|choice| written.contains(&choice.name)).map(|choice| choice.id.to_string()).collect();
        let others: Vec<String> = kition_table_file::kdm::cells::all_choices(file.replica(), &field)
            .into_iter()
            .map(|choice| choice.to_string())
            .filter(|choice| !named.contains(choice))
            .collect();
        needs.select_labels(id, &others);
    }
}

fn creation_names(file: &TableFile, needs: &mut Needs, selected: &mut Selection, method: &str, params: &Value) {
    let table = params["tableId"].as_i64().unwrap_or_default();
    let table_creation =
        method == "tables.create" || (method == "import.run" && matches!(params["target"]["kind"].as_str(), Some("new_table" | "new_document")));
    let title = match method {
        "tables.create" => params["title"].as_str().filter(|title| !title.trim().is_empty()).unwrap_or("Table").to_owned(),
        "import.run" if table_creation => params["target"]["table_title"].as_str().unwrap_or("Table").to_owned(),
        "fields.create" => params["title"].as_str().filter(|title| !title.trim().is_empty()).unwrap_or("Field").to_owned(),
        "fields.duplicate" => {
            let Ok(field) = file.get_field(table, params["fieldId"].as_i64().unwrap_or_default()) else { return };
            format!("{} copy", field.title)
        }
        _ => return,
    };
    let explicit = params["name"].as_str().filter(|name| !name.trim().is_empty());
    let base = kition_table_file::internal_name(explicit.unwrap_or(&title));
    let objects: Vec<(String, String)> = if table_creation {
        file.list_tables().unwrap_or_default().into_iter().filter_map(|t| Some((resolve(file, "Table", &json!(t.id))?, t.name))).collect()
    } else {
        file.list_fields(table).unwrap_or_default().into_iter().filter_map(|f| Some((resolve(file, "Field", &json!(f.id))?, f.name))).collect()
    };
    let mut candidates = BTreeSet::new();
    let mut name = base.clone();
    let mut suffix = 2;
    loop {
        candidates.insert(name.clone());
        if explicit.is_some() || !objects.iter().any(|(_, taken)| *taken == name) {
            break;
        }
        name = format!("{base}_{suffix}");
        suffix += 1;
    }
    for (id, name) in objects {
        if candidates.contains(&name) {
            needs.add(id.clone(), vec!["name".into()]);
            if !table_creation {
                selected.owners.insert(id);
            }
        }
    }
    if table_creation {
        selected.table_names.extend(candidates);
    } else {
        selected.generated_names.entry(table).or_default().extend(candidates);
    }
}

/// What a field's formula and options name: fields of its table by name (membership), and
/// tables and fields by id (that they are there). The cycle check reads more of them, but it
/// only refuses the call.
fn schema(file: &TableFile, needs: &mut Needs, selected: &mut Selection, table: i64, formula: Option<&str>, options: &Value) {
    if let Some(formula) = formula {
        for name in kition_table_file::formula_references(formula) {
            selected.name(table, &name);
        }
    }
    for key in ["link_field_id", "target_field_id", "symmetric_field_id"] {
        if let Some(id) = options[key].as_i64().and_then(|id| resolve(file, "Field", &json!(id))) {
            needs.exists(id);
        }
    }
    if let Some(id) = options["target_table_id"].as_i64().and_then(|id| resolve(file, "Table", &json!(id))) {
        needs.exists(id);
    }
    let config = &options["ai_config"];
    if let Some(name) = config["source_field_name"].as_str().filter(|s| !s.is_empty()) {
        selected.name(table, name);
    }
    if let Some(id) = config["source_field_id"].as_i64().and_then(|id| resolve(file, "Field", &json!(id))) {
        needs.exists(id);
    }
}

fn values(file: &TableFile, needs: &mut Needs, selected: &mut Selection, table: i64, record: Option<String>, values: &Value) {
    for (name, value) in values.as_object().into_iter().flatten() {
        selected.name(table, name);
        if let Some(field) = file.list_fields(table).unwrap_or_default().iter().find(|f| f.name == *name && f.field_type == "link_to_record") {
            let mut targets = Vec::new();
            for value in value.as_array().map(Vec::as_slice).unwrap_or(std::slice::from_ref(value)).iter().filter(|v| !v.is_null()) {
                let value = value.get("row_key").unwrap_or(value);
                selected.windows.extend(crate::address::window(file, "Record", value));
                if let Some(id) = resolve(file, "Record", value) {
                    needs.add(id.clone(), Vec::new());
                    selected.owners.insert(id);
                    targets.push(resolve(file, "Record", value).unwrap_or_default());
                }
            }
            link_write(file, needs, selected, field, record.as_deref(), &targets);
        }
    }
}

fn symmetric(file: &TableFile, field: &Field) -> Option<Field> {
    let mut fields: Vec<_> = file.list_tables().ok()?.into_iter().flat_map(|t| t.fields).filter(|f| f.field_type == "link_to_record").collect();
    fields.sort_by_key(|f| resolve(file, "Field", &json!(f.id)));
    let declared = field.options.get("symmetric_field_id").and_then(Value::as_i64).and_then(|id| fields.iter().find(|f| f.id == id));
    let partner = declared
        .or_else(|| fields.iter().find(|f| f.id != field.id && f.options.get("symmetric_field_id").and_then(Value::as_i64) == Some(field.id)))?;
    (partner.options.get("target_table_id").and_then(Value::as_i64) == Some(field.table_id)
        && field.options.get("target_table_id").and_then(Value::as_i64) == Some(partner.table_id))
    .then(|| partner.clone())
}

fn link_write(file: &TableFile, needs: &mut Needs, selected: &mut Selection, field: &Field, record: Option<&str>, targets: &[String]) {
    let Some(field_id) = resolve(file, "Field", &json!(field.id)) else { return };
    // A link is written by its field's type and options (target, single or multiple, partner).
    needs.add(field_id.clone(), strings(&["options", "type"]));
    selected.mirrors.insert(field_id.clone());
    if let Some(record) = record {
        selected.links.insert((record.to_owned(), field_id.clone()));
    }
    let Some(partner) = symmetric(file, field) else { return };
    let Some(partner_id) = resolve(file, "Field", &json!(partner.id)) else { return };
    needs.add(partner_id.clone(), strings(&["options", "type"]));
    selected.owners.insert(partner_id.clone());
    let mut touched: BTreeSet<_> = targets.iter().cloned().collect();
    for link in file.replica().objects_of_type("Link") {
        if file.replica().get(&link, "field").ok().as_ref() == Some(&json!(field_id))
            && file.replica().get(&link, "from").ok().as_ref().and_then(Value::as_str) == record
            && let Ok(Value::String(to)) = file.replica().get(&link, "to")
        {
            touched.insert(to);
        }
    }
    for target in touched {
        selected.links.insert((target, partner_id.clone()));
    }
}

fn source_value(file: &TableFile, needs: &mut Needs, selected: &mut Selection, field: &Field, record: &str, seen: &mut BTreeSet<(i64, String)>) {
    if !seen.insert((field.id, record.to_owned())) {
        return;
    }
    let Some(id) = definition(file, needs, field) else { return };
    needs.add(record.to_owned(), vec![format!("cells.{id}")]);
    selected.owners.insert(record.to_owned());
    let fields = file.list_fields(field.table_id).unwrap_or_default();
    if field.field_type == "formula" {
        for name in kition_table_file::formula_references(&field.formula) {
            selected.name(field.table_id, &name);
            if let Some(source) = fields.iter().find(|f| f.name == name) {
                source_value(file, needs, selected, source, record, seen);
            }
        }
    }
    let link = if field.field_type == "link_to_record" {
        Some(field.id)
    } else if matches!(field.field_type.as_str(), "lookup" | "rollup") {
        field.options.get("link_field_id").and_then(Value::as_i64)
    } else {
        None
    };
    if let Some(link) = link.and_then(|id| fields.iter().find(|f| f.id == id)) {
        let Some(link_id) = definition(file, needs, link) else { return };
        selected.links.insert((record.to_owned(), link_id.clone()));
        let target = if field.field_type == "link_to_record" {
            link.options
                .get("target_table_id")
                .and_then(Value::as_i64)
                .and_then(|t| file.get_table(t).ok())
                .and_then(|t| t.fields.into_iter().find(|f| f.is_primary))
        } else {
            let target = field.options.get("target_field_id").and_then(Value::as_i64);
            file.list_tables().unwrap_or_default().into_iter().flat_map(|t| t.fields).find(|f| Some(f.id) == target)
        };
        for object in file.replica().objects_of_type("Link") {
            let get = |key| file.replica().get(&object, key).unwrap_or(Value::Null);
            if get("from") == record
                && get("field") == link_id
                && let Some(to) = get("to").as_str()
            {
                needs.add(to.to_owned(), Vec::new());
                selected.owners.insert(to.to_owned());
                if let Some(target) = &target {
                    source_value(file, needs, selected, target, to, seen);
                } else if field.field_type == "rollup"
                    && field.options.get("aggregation").and_then(Value::as_str) == Some("concat")
                    && let Some(table) = link.options.get("target_table_id").and_then(Value::as_i64)
                {
                    for source in table_schema(file, needs, selected, table) {
                        source_value(file, needs, selected, &source, to, seen);
                    }
                }
            }
        }
    }
}

/// The objects a view's or dashboard's settings name: that they are there, and `read` of a field
/// they name by id; names of the table's fields by membership.
fn references(file: &TableFile, needs: &mut Needs, selected: &mut Selection, table: i64, value: &Value, read: &[&str]) {
    match value {
        Value::Array(items) => {
            for item in items {
                references(file, needs, selected, table, item, read);
            }
        }
        Value::Object(map) => {
            let table = ["tableId", "table_id", "source_table_id", "sourceTableId"]
                .iter()
                .find_map(|key| map.get(*key).and_then(Value::as_i64))
                .unwrap_or(table);
            for (key, value) in map {
                if let Some(types) = crate::params::kind(key) {
                    for ty in types {
                        for value in value.as_array().map(Vec::as_slice).unwrap_or(std::slice::from_ref(value)) {
                            if let Some(id) = resolve(file, ty, value) {
                                if matches!(*ty, "Field" | "View" | "Record") {
                                    selected.owners.insert(id.clone());
                                }
                                needs.exists(id.clone());
                                needs.add(id, if *ty == "Field" { strings(read) } else { Vec::new() });
                            }
                        }
                    }
                }
                if matches!(key.as_str(), "field_name" | "group_field_name" | "cover_field_name" | "group_by_field_name" | "source_field_name") {
                    if let Some(name) = value.as_str() {
                        selected.name(table, name);
                    }
                } else if matches!(key.as_str(), "columns" | "hidden_field_names") {
                    for name in value.as_array().into_iter().flatten().filter_map(Value::as_str) {
                        selected.name(table, name);
                    }
                } else {
                    references(file, needs, selected, table, value, read);
                }
            }
        }
        _ => {}
    }
}

fn table_schema(file: &TableFile, needs: &mut Needs, selected: &mut Selection, table: i64) -> Vec<Field> {
    if let Some(id) = resolve(file, "Table", &json!(table)) {
        selected.schema_tables.insert(id);
    }
    let fields = file.list_fields(table).unwrap_or_default();
    for field in &fields {
        selected.name(table, &field.name);
        // A column finds its field by name or title, in field order; its text is converted by
        // the field's type (a link's by its options). The required check only refuses.
        let read: &[&str] = if field.field_type == "link_to_record" { &["name", "title", "type", "options"] } else { &["name", "title", "type"] };
        if let Some(id) = resolve(file, "Field", &json!(field.id)) {
            needs.add(id, strings(read));
        }
    }
    fields
}

/// A copy's stored cells: which fields are stored is their types.
fn stored_fields(file: &TableFile, needs: &mut Needs, table: i64) {
    for field in file.list_fields(table).unwrap_or_default() {
        if let Some(id) = resolve(file, "Field", &json!(field.id)) {
            needs.add(id, strings(&["type"]));
        }
    }
}

fn strings(items: &[&str]) -> Vec<String> {
    items.iter().map(|item| (*item).to_owned()).collect()
}

fn import(file: &TableFile, needs: &mut Needs, selected: &mut Selection, params: &Value) {
    use kition_table_import::{ExecuteRequest, SourceParams, write};
    let Ok(request) = serde_json::from_value::<ExecuteRequest>(params.clone()) else { return };
    let table = request.target.table_id;
    for field in &request.field_overrides {
        schema(file, needs, selected, table, None, &json!(field.options));
    }
    if request.target.kind != "existing_table" {
        return;
    }
    let Some(id) = resolve(file, "Table", &json!(table)) else { return };
    needs.set(id.clone(), Vec::new(), Vec::new());
    let fields = table_schema(file, needs, selected, table);
    let schema_replaced = request.schema_mode == "replace"
        || (request.schema_mode == "auto"
            && matches!(fields.as_slice(),[a,b,c] if a.title == "Title" && a.field_type == "text" && b.title == "Status" && b.field_type == "single_select" && c.title == "Notes" && c.field_type == "long_text"));
    // A replaced schema deletes the fields past the source's columns except the primary one.
    if schema_replaced {
        let columns = serde_json::from_value::<SourceParams>(params.clone())
            .ok()
            .and_then(|p| kition_table_import::read_source(&p).ok())
            .and_then(|s| kition_table_import::parse_source(&s).ok())
            .map_or(0, |(preview, _)| preview.fields.len());
        if fields.len() > columns {
            needs.add(id.clone(), strings(&["primaryField"]));
        }
    }
    let replace = request.write_mode == "replace" || schema_replaced;
    if replace || request.write_mode == "upsert" {
        selected.row_tables.insert(id);
    }
    let Ok(records) = file.list_records(table) else { return };
    if replace {
        for record in records {
            selected.record_links.insert(record.row_key.clone());
            needs.full(record.row_key);
        }
        return;
    }
    if request.write_mode != "upsert" {
        return;
    }
    let source = serde_json::from_value::<SourceParams>(params.clone())
        .ok()
        .and_then(|p| kition_table_import::read_source(&p).ok())
        .and_then(|s| kition_table_import::parse_source(&s).ok());
    let Some((preview, rows)) = source else { return };
    let source_fields = write::apply_overrides(&preview.fields, &request.field_overrides);
    let keys: BTreeSet<String> = rows
        .iter()
        .skip(1)
        .map(|row| write::values_key(&fields, &write::row_values(&fields, &source_fields, row), &request.unique_by))
        .filter(|key| !key.is_empty())
        .collect();
    for record in records {
        for field in fields
            .iter()
            .filter(|f| request.unique_by.iter().any(|key| f.name.eq_ignore_ascii_case(key.trim()) || f.title.eq_ignore_ascii_case(key.trim())))
        {
            source_value(file, needs, selected, field, &record.row_key, &mut BTreeSet::new());
        }
        let key = write::values_key(&fields, &record.values, &request.unique_by);
        if keys.contains(&key) {
            for field in fields.iter().filter(|f| source_fields.iter().any(|source| source.title.trim().eq_ignore_ascii_case(f.title.trim()))) {
                source_value(file, needs, selected, field, &record.row_key, &mut BTreeSet::new());
            }
        }
    }
}

pub(crate) fn refine(file: &TableFile, method: &str, params: &Value, needs: &mut Needs) -> Selection {
    let mut selected = Selection::default();
    let table = params["tableId"].as_i64().unwrap_or_default();
    creation_names(file, needs, &mut selected, method, params);
    match method {
        "tables.create" => {
            for field in params["fields"].as_array().into_iter().flatten() {
                schema(
                    file,
                    needs,
                    &mut selected,
                    table,
                    (field["type"] == "formula").then(|| field["formula"].as_str().unwrap_or_default()),
                    &field["options"],
                );
            }
        }
        "import.run" => import(file, needs, &mut selected, params),
        "csv.import" => {
            table_schema(file, needs, &mut selected, table);
        }
        "records.bulkCopy" => stored_fields(file, needs, table),
        "records.create" | "records.update" => {
            values(file, needs, &mut selected, table, params.get("recordId").and_then(|v| resolve(file, "Record", v)), &params["values"])
        }
        "transact" => {
            for op in params["ops"].as_array().into_iter().flatten() {
                let table = op["tableId"].as_i64().unwrap_or_default();
                for row in op["records"].as_array().into_iter().flatten() {
                    match op["op"].as_str() {
                        Some("cells.set") => {
                            values(file, needs, &mut selected, table, row.get("rowKey").and_then(|v| resolve(file, "Record", v)), &row["values"])
                        }
                        Some("records.create") => values(file, needs, &mut selected, table, None, row),
                        _ => {}
                    }
                }
                if op["op"] == "records.delete" {
                    selected.record_links.extend(crate::params::values(op, "rowKeys").into_iter().filter_map(|value| resolve(file, "Record", value)));
                }
            }
        }
        "links.replace" | "links.delete" => {
            for value in crate::params::values(params, "target_row_keys") {
                if let Some(id) = resolve(file, "Record", value) {
                    needs.set(id, Vec::new(), Vec::new());
                }
            }
            if let (Some(record), Some(field)) = (resolve(file, "Record", &params["recordId"]), resolve(file, "Field", &params["field_id"])) {
                needs.set(record.clone(), Vec::new(), Vec::new());
                needs.exists(field.clone());
                selected.links.insert((record, field));
            }
            if let Ok(field) = file.get_field(table, params["field_id"].as_i64().unwrap_or_default()) {
                let targets: Vec<_> =
                    crate::params::values(params, "target_row_keys").into_iter().filter_map(|v| resolve(file, "Record", v)).collect();
                let record = resolve(file, "Record", &params["recordId"]);
                link_write(file, needs, &mut selected, &field, record.as_deref(), &targets);
            }
        }
        "fields.create" | "fields.update" => {
            // What the call writes names (its options and formula); an update leaves the stored
            // ones as they are, and a duplicate copies its source whole (held whole).
            let existing = params["fieldId"].as_i64().and_then(|field| file.get_field(table, field).ok());
            let ty = params["type"].as_str().or_else(|| existing.as_ref().map(|f| f.field_type.as_str())).unwrap_or("text");
            let formula = params["formula"].as_str();
            schema(file, needs, &mut selected, table, formula.filter(|_| ty == "formula"), &params["options"]);
        }
        "ai.apply" => {
            let field_id = params["fieldId"].as_i64().unwrap_or_default();
            let fields = file.list_fields(table).unwrap_or_default();
            if let Some(field) = fields.iter().find(|f| f.id == field_id) {
                schema(file, needs, &mut selected, table, (field.field_type == "formula").then_some(field.formula.as_str()), &json!(field.options));
            }
            if let Ok((field, source, _)) = kition_table_ai::resolve_field(&fields, field_id) {
                if let Some(id) = resolve(file, "Field", &json!(field.id)) {
                    needs.set(id, ["name", "title", "type", "options", "aiPrompt"].map(str::to_owned).to_vec(), Vec::new());
                }
                for id in crate::params::values(params, "results[].recordId").into_iter().chain(params.get("recordId")) {
                    if let Some(record) = resolve(file, "Record", id) {
                        let mut cells = vec![format!("cells.{}", resolve(file, "Field", &json!(field.id)).unwrap_or_default())];
                        if let Some(source) = &source {
                            if let Some(id) = definition(file, needs, source) {
                                needs.add(id.clone(), vec!["title".into()]);
                                cells.push(format!("cells.{id}"));
                                if source.field_type == "link_to_record" {
                                    selected.links.insert((record.clone(), id));
                                }
                            }
                            selected.name(table, &source.name);
                        }
                        needs.set(record, cells, Vec::new());
                        if let Some(source) = &source {
                            source_value(file, needs, &mut selected, source, &resolve(file, "Record", id).unwrap_or_default(), &mut BTreeSet::new());
                        }
                    }
                }
            }
        }
        _ => {}
    }
    if method.starts_with("views.") {
        // A view's config is stored as given; a filter, sort or group writes its field's name
        // beside its id.
        references(file, needs, &mut selected, table, &params["config"], &[]);
        for key in ["tree", "items"] {
            references(file, needs, &mut selected, table, &params[key], &["name"]);
        }
    }
    if method == "dashboards.upsert" {
        // Widgets are stored as given: the fields they name by id are there, those by name are
        // the table's names.
        let table = params["source_table_id"].as_i64().unwrap_or_default();
        references(file, needs, &mut selected, table, &params["widgets"], &[]);
    }
    // A type change writes the stored cells again in the new type's shape, from what they hold
    // when it runs, so it does not rest on them; a delete takes them.
    if method == "fields.delete"
        && let (Some(table), Some(field)) = (resolve(file, "Table", &params["tableId"]), resolve(file, "Field", &params["fieldId"]))
    {
        selected.row_tables.insert(table);
        selected.field_links.insert(field.clone());
        for record in file.list_records(params["tableId"].as_i64().unwrap_or_default()).unwrap_or_default() {
            needs.add(record.row_key, vec![format!("cells.{field}")]);
        }
    }
    if matches!(method, "records.delete" | "records.bulkDelete") {
        for value in params.get("recordId").into_iter().chain(crate::params::values(params, "row_keys")) {
            if let Some(id) = resolve(file, "Record", value) {
                selected.record_links.insert(id);
            }
        }
    }
    if method == "tables.delete"
        && let Some(table) = params["tableId"].as_i64()
    {
        selected.record_links.extend(file.list_records(table).unwrap_or_default().into_iter().map(|r| r.row_key));
        selected.field_links.extend(file.list_fields(table).unwrap_or_default().iter().filter_map(|f| resolve(file, "Field", &json!(f.id))));
        if let Some(id) = resolve(file, "Table", &json!(table)) {
            for list in ["fields", "views", "records"] {
                for child in file.replica().list(&ObjectId::from_string(&id), list).unwrap_or_default() {
                    needs.full(child.to_string());
                }
            }
        }
    }
    let mut labels = Labels::new();
    match method {
        "records.create" | "records.update" => select_labels(file, &mut labels, table, &params["values"]),
        "transact" => {
            for op in params["ops"].as_array().into_iter().flatten() {
                for row in op["records"].as_array().into_iter().flatten() {
                    match op["op"].as_str() {
                        Some("cells.set") => select_labels(file, &mut labels, op["tableId"].as_i64().unwrap_or_default(), &row["values"]),
                        Some("records.create") => select_labels(file, &mut labels, op["tableId"].as_i64().unwrap_or_default(), row),
                        _ => {}
                    }
                }
            }
        }
        _ => {}
    }
    narrow_select_labels(file, needs, &labels);
    selected
}

/// A table's field order as a small value (`~fieldOrder`): the journal keeps the `before` of a
/// derived value only while it is small, so each table holds a hash of its own ordered field
/// identities rather than one root map of every list. Windows of local addresses are the
/// guard's (`within`), not a derived fact.
pub(crate) fn field_order(file: &TableFile, table: &ObjectId) -> Value {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for id in file.replica().list(table, "fields").unwrap_or_default() {
        for byte in id.as_str().bytes().chain(*b"\n") {
            hash ^= u64::from(byte);
            hash = hash.wrapping_mul(0x0100_0000_01b3);
        }
    }
    json!(format!("{hash:016x}"))
}

pub(crate) fn derived(file: &TableFile, ids: &[Value], names: &[String]) -> Vec<kition_kit::DerivedValues> {
    let wanted = |id: &str| ids.is_empty() || ids.iter().any(|v| v.as_str() == Some(id));
    let mut result = Vec::new();
    if names.is_empty() || names.iter().any(|n| n == "fieldOrder") {
        for table in file.replica().objects_of_type("Table").into_iter().filter(|id| wanted(id.as_str())) {
            let values = serde_json::Map::from_iter([("fieldOrder".to_owned(), field_order(file, &table))]);
            result.push(kition_kit::DerivedValues { id: json!(table.as_str()), values });
        }
    }
    let Some(root) = file.replica().objects_of_type("Base").first().cloned() else { return result };
    if !wanted(root.as_str()) {
        return result;
    }
    let mut rows = serde_json::Map::new();
    for table in file.list_tables().unwrap_or_default() {
        let Some(id) = resolve(file, "Table", &json!(table.id)) else { continue };
        let mut records: Vec<String> =
            file.replica().list(&ObjectId::from_string(&id), "records").unwrap_or_default().into_iter().map(|r| r.to_string()).collect();
        records.sort();
        rows.insert(id, json!(records));
    }
    let mut links: BTreeMap<String, BTreeMap<String, BTreeSet<String>>> = BTreeMap::new();
    let mirrors: BTreeMap<_, _> = file
        .list_tables()
        .unwrap_or_default()
        .into_iter()
        .flat_map(|t| t.fields)
        .filter(|f| f.field_type == "link_to_record")
        .filter_map(|field| {
            let id = resolve(file, "Field", &json!(field.id))?;
            let partner = symmetric(file, &field).and_then(|f| resolve(file, "Field", &json!(f.id)));
            Some((id, partner))
        })
        .collect();
    for id in file.replica().objects_of_type("Link") {
        let get = |key| file.replica().get(&id, key).ok().and_then(|v| v.as_str().map(str::to_owned));
        if let (Some(from), Some(field), Some(to)) = (get("from"), get("field"), get("to")) {
            links.entry(from).or_default().entry(field).or_default().insert(to);
        }
    }
    let values = [("linkCells", json!(links)), ("mirrors", json!(mirrors)), ("rowIds", json!(rows))]
        .into_iter()
        .filter(|(name, _)| names.is_empty() || names.iter().any(|n| n == name))
        .map(|(name, value)| (name.to_owned(), value))
        .collect();
    result.push(kition_kit::DerivedValues { id: json!(root.as_str()), values });
    result
}

pub(crate) fn owner(file: &TableFile, id: &str) -> Value {
    let ty = file.replica().object_type(&ObjectId::from_string(id)).ok();
    let Some((table, column)) = ty.as_deref().and_then(crate::address::row).filter(|(table, _)| *table != "kition_tables") else {
        return Value::Null;
    };
    file.conn().query_row(&format!("SELECT t.kdm_id FROM {table} child JOIN kition_tables t ON t.id = child.table_id WHERE child.{column} = ?1 AND child.deleted_at IS NULL AND t.deleted_at IS NULL"),[id],|row|row.get::<_,String>(0)).map(Value::String).unwrap_or(Value::Null)
}

pub(crate) fn covered(file: &TableFile, selected: &Selection, asks: &[CoveredAsk]) -> Result<CoveredAt, String> {
    // A reread already removed links to a deleted input. Recover those identities from
    // the historical membership so the refusal can name the deleted record itself.
    let deleted_inputs: BTreeSet<String> =
        asks.iter()
            .filter_map(|ask| ask.before.get("~linkCells"))
            .flat_map(|before| {
                selected.links.iter().flat_map(move |(record, field)| {
                    before[record][field].as_array().into_iter().flatten().filter_map(Value::as_str).map(str::to_owned)
                })
            })
            .collect();
    let mut under: BTreeSet<String> = selected.objects(file).into_iter().filter_map(|id| id.as_str().map(str::to_owned)).collect();
    for w in &selected.windows {
        if w.from != w.to
            && let Some(parent) = w.parent.as_str()
            && let Ok(items) = file.replica().list(&ObjectId::from_string(parent), &w.field)
        {
            under.extend(items.iter().skip(w.from).take(w.to - w.from).map(|id| id.to_string()));
        }
    }
    let window_objects: BTreeSet<String> = selected
        .windows
        .iter()
        .filter(|w| w.from != w.to)
        .flat_map(|w| {
            w.parent
                .as_str()
                .and_then(|parent| file.replica().list(&ObjectId::from_string(parent), &w.field).ok())
                .unwrap_or_default()
                .into_iter()
                .skip(w.from)
                .take(w.to - w.from)
                .map(|id| id.to_string())
        })
        .collect();
    let mut covered = Vec::new();
    for ask in asks {
        let Some(id) = ask.id.as_str() else { continue };
        let object = ObjectId::from_string(id);
        let ty = file.replica().object_type(&object).ok();
        // A call reading a table's schema rests on its field order (`~fieldOrder`, one small
        // value per table, so the journal keeps its before).
        let schema = if !selected.schema_tables.contains(id) || ask.created {
            Some(false)
        } else if ask.deleted || ty.is_none() {
            Some(true)
        } else if ask.unknown.iter().any(|key| key == "~fieldOrder" || key == "$was") {
            None
        } else {
            Some(ask.before.get("~fieldOrder").is_some_and(|before| *before != field_order(file, &object)))
        };
        let was = if ask.created {
            Some(false)
        } else if ask.deleted && deleted_inputs.contains(id) {
            Some(true)
        } else if ty.as_deref() == Some("Base") {
            let now = derived(file, &[json!(id)], &[]).into_iter().find(|v| v.id == json!(id)).map(|v| v.values).unwrap_or_default();
            let relevant = |key: &str| match key {
                "~linkCells" => !selected.links.is_empty() || !selected.record_links.is_empty() || !selected.field_links.is_empty(),
                "~mirrors" => !selected.mirrors.is_empty(),
                "~rowIds" => !selected.row_tables.is_empty(),
                _ => false,
            };
            if ask.unknown.iter().any(|key| relevant(key)) {
                None
            } else {
                let changed = ask.before.iter().any(|(key, before)| {
                    let after = &now[key.trim_start_matches('~')];
                    match key.as_str() {
                        "~mirrors" => selected.mirrors.iter().any(|id| before[id] != after[id]),
                        "~linkCells" => {
                            let scope = |value: &Value| -> Value {
                                value
                                    .as_object()
                                    .into_iter()
                                    .flatten()
                                    .map(|(record, cells)| {
                                        let cells: serde_json::Map<String, Value> = cells
                                            .as_object()
                                            .into_iter()
                                            .flatten()
                                            .filter(|(field, targets)| {
                                                selected.links.contains(&(record.clone(), (*field).clone()))
                                                    || selected.record_links.contains(record)
                                                    || selected.field_links.contains(*field)
                                                    || targets
                                                        .as_array()
                                                        .into_iter()
                                                        .flatten()
                                                        .filter_map(Value::as_str)
                                                        .any(|id| selected.record_links.contains(id))
                                            })
                                            .map(|(field, targets)| (field.clone(), targets.clone()))
                                            .collect();
                                        (record.clone(), Value::Object(cells))
                                    })
                                    .filter(|(_, cells)| cells.as_object().is_some_and(|cells| !cells.is_empty()))
                                    .collect::<serde_json::Map<_, _>>()
                                    .into()
                            };
                            scope(before) != scope(after)
                        }
                        "~rowIds" => selected.row_tables.iter().any(|table| before[table] != after[table]),
                        _ => false,
                    }
                });
                Some(changed)
            }
        } else if ty.as_deref() == Some("Link") || ask.before.contains_key("from") {
            let old = |key| ask.before.get(key).cloned().or_else(|| file.replica().get(&object, key).ok());
            match (old("from"), old("field")) {
                (Some(Value::String(from)), Some(Value::String(field))) => Some(selected.links.contains(&(from, field))),
                _ if selected.links.is_empty() => Some(false),
                _ => None,
            }
        } else if ty.as_deref() == Some("Table")
            || (!ask.before.contains_key("~owner") && !selected.table_names.is_empty() && ask.before.contains_key("name"))
        {
            if ask.unknown.iter().any(|key| key == "name" || key == "$was") {
                None
            } else {
                ask.before
                    .get("name")
                    .cloned()
                    .or_else(|| file.replica().get(&object, "name").ok())
                    .and_then(|value| value.as_str().map(|name| selected.table_names.contains(name)))
            }
        } else if ty.as_deref() == Some("Field") || ask.before.contains_key("name") {
            let owner = ask.before.get("~owner").cloned().unwrap_or_else(|| owner(file, id));
            let names: BTreeSet<_> = selected
                .names
                .iter()
                .chain(&selected.generated_names)
                .filter(|(table, _)| resolve(file, "Table", &json!(table)).as_deref() == owner.as_str())
                .flat_map(|(_, names)| names.iter().cloned())
                .collect();
            match names {
                names if names.is_empty() => Some(false),
                _ if ask.unknown.iter().any(|f| f == "name" || f == "~owner" || f == "$was") => None,
                names => ask
                    .before
                    .get("name")
                    .cloned()
                    .or_else(|| file.replica().get(&object, "name").ok())
                    .and_then(|v| v.as_str().map(|name| names.contains(name))),
            }
        } else {
            Some(false)
        };
        let was = match schema {
            Some(true) => Some(true),
            None => None,
            Some(false) => was,
        };
        let was = was.map(|was| was || window_objects.contains(id));
        under.remove(id);
        if was == Some(true) {
            under.insert(id.to_owned());
        }
        covered.push(Covered { id: ask.id.clone(), covered: was, gone: ask.deleted || ty.is_none(), content: None });
    }
    Ok(CoveredAt { under: under.into_iter().map(|id| json!(id)).collect(), covered })
}
