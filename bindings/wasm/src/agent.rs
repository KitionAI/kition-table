//! What agents read about each command (KHP 1.0, `spec/agent.md` sections 1.2 and 1.3): its
//! parameters as a `ParameterSchemaSpec` (A5, A6) and its answer as a `ValueSchemaSpec` (A1),
//! with the JSON Schema an MCP binding offers as `inputSchema` and `outputSchema` (section 8).
//!
//! The schemas come from the Rust types the requests deserialize into (`schemars`), converted to
//! the spec's dialect by [`from_json_schema`]: the route's ids from its template in
//! [`METHODS`], its body from `kition-table-api`'s request types, and the import's from
//! `kition-table-import`'s. The bodies the engine reads key by key (`tables.update`,
//! `fields.update`, `views.update`, `dashboards.upsert`) are described by the types below, which
//! list exactly the keys their handlers read. The AI field methods and `changes.since` read loose
//! JSON and are written out here.
//!
//! Every output schema accepts the error result (A18): `result`, `change`, and `error` are all
//! optional top-level properties, so `{ "error": { code, message, index?, retryable? } }` is as
//! valid as `{ "result", "change" }`.

use kition_table_api::{METHODS, records, tables, transact, view_config};
use kition_table_file::{FIELD_TYPES, VIEW_TYPES};
use schemars::{JsonSchema, Schema, schema_for};
use serde::Deserialize;
use serde_json::{Map, Value, json};

use crate::{AS_ORIGIN_PARAM, COMMAND_PREFIX, DOCUMENT_METHODS, ENGINE_METHODS, READ_METHODS};

/// The tool name prefix (`agent.prefix` in `kit.json`).
pub const TOOL_PREFIX: &str = "table";

/// How many types deep [`from_json_schema`] inlines `$ref`s (the filter tree refers to itself);
/// deeper values are `json`.
const MAX_DEPTH: usize = 3;

/// The keys `tables.update` reads (`kition_table_api::tables::update_table`).
#[allow(dead_code)]
#[derive(Deserialize, JsonSchema)]
struct TableUpdate {
    /// The table's new title.
    title: Option<String>,
    /// The table's new description.
    description: Option<String>,
    /// Free-form metadata the host keeps with the table.
    meta: Option<Value>,
    /// The id of the field that becomes the primary field.
    primary_field_id: Option<i64>,
}

/// The keys `fields.update` reads (`kition_table_api::tables::update_field`).
#[allow(dead_code)]
#[derive(Deserialize, JsonSchema)]
struct FieldUpdate {
    title: Option<String>,
    /// The field type, such as `text`, `long_text`, `number`, `single_select`, `date`.
    #[serde(rename = "type")]
    field_type: Option<String>,
    required: Option<bool>,
    readonly: Option<bool>,
    is_primary: Option<bool>,
    /// The type's options, such as `{ "choices": [...] }` for a select.
    options: Option<Value>,
    formula: Option<String>,
    ai_prompt: Option<String>,
    meta: Option<Value>,
}

/// The keys `views.update` reads (`kition_table_api::tables::update_view`).
#[allow(dead_code)]
#[derive(Deserialize, JsonSchema)]
struct ViewUpdate {
    title: Option<String>,
    /// The view type: `grid`, `kanban`, `gallery`, or `calendar`.
    #[serde(rename = "type")]
    view_type: Option<String>,
    /// The view's configuration object.
    config: Option<Value>,
}

/// The keys `dashboards.upsert` reads (`kition_table_api::dashboards::dashboard_from_request`).
#[allow(dead_code)]
#[derive(Deserialize, JsonSchema)]
struct DashboardBody {
    title: Option<String>,
    order: Option<f64>,
    source_table_id: Option<i64>,
    /// The widgets' positions.
    layout: Option<Vec<Value>>,
    /// The widgets, each an object with its own `id`.
    widgets: Option<Vec<Map<String, Value>>>,
}

/// The params of `attachments.put` (`kition_table_api::attachments::PutRequest`), renamed
/// camelCase as it is.
type PutRequest = kition_table_api::attachments::PutRequest;

/// The params of `import.run`: the data-import execute request and the file it reads
/// (`kition_table_import::run`).
#[allow(dead_code)]
#[derive(Deserialize, JsonSchema)]
struct RunParams {
    #[serde(flatten)]
    request: kition_table_import::ExecuteRequest,
    #[serde(flatten)]
    source: kition_table_import::SourceParams,
}

/// The query values a route reads beside its ids and body.
fn query_params(method: &str) -> Vec<(&'static str, Value)> {
    let limit = || json!({ "type": "integer", "description": "How many items to answer at most." });
    let offset = || json!({ "type": "integer", "description": "How many items to skip, from 0." });
    match method {
        "records.list" => vec![("limit", limit()), ("offset", offset())],
        "records.history" => vec![
            ("limit", limit()),
            ("offset", offset()),
            (
                "checkpoints",
                json!({
                    "type": "array",
                    "description": "The host's checkpoints of the document, oldest first (Kit UI protocol 7m): the history before this session is told between them.",
                    "items": {
                        "type": "object",
                        "additionalProperties": false,
                        "properties": {
                            "version": { "type": "string", "required": true, "description": "The replica version, base64, as replica/version answers it." },
                            "time": { "type": "integer", "description": "When it was recorded, in milliseconds since the Unix epoch." },
                            "author": { "type": "string", "description": "Who made it, when the host knows." }
                        }
                    }
                }),
            ),
        ],
        "fields.linkCandidates" => {
            vec![("q", json!({ "type": "string", "description": "Text to match in the candidates' display values." })), ("limit", limit())]
        }
        "links.list" => vec![("field_id", json!({ "type": "integer", "required": true, "description": "The link field's id." }))],
        _ => Vec::new(),
    }
}

/// The JSON Schema of a method's body, from the Rust type it deserializes into.
fn body_schema(method: &str) -> Option<Schema> {
    Some(match method {
        "tables.create" => schema_for!(tables::TableSeed),
        "tables.update" => schema_for!(TableUpdate),
        "fields.create" => schema_for!(tables::CreateFieldRequest),
        "fields.update" => schema_for!(FieldUpdate),
        "views.create" => schema_for!(tables::ViewSeed),
        "views.update" => schema_for!(ViewUpdate),
        "views.putFilterTree" => schema_for!(view_config::TreeRequest),
        "views.putSorts" | "views.putGroups" => schema_for!(view_config::ItemsRequest),
        "views.patchConfig" => schema_for!(view_config::ConfigPatch),
        "views.patchField" => schema_for!(view_config::ViewFieldRequest),
        "records.create" | "records.update" => schema_for!(records::RecordRequest),
        "records.bulkDelete" | "records.bulkCopy" => schema_for!(view_config::BulkRequest),
        "links.replace" | "links.delete" => schema_for!(records::LinkRequest),
        "csv.import" => schema_for!(records::CsvRequest),
        "transact" => schema_for!(transact::TransactRequest),
        "dashboards.upsert" => schema_for!(DashboardBody),
        "attachments.put" => schema_for!(PutRequest),
        "import.preview" => schema_for!(kition_table_import::SourceParams),
        "import.run" => schema_for!(RunParams),
        _ => return None,
    })
}

/// The params of the methods whose requests are read as loose JSON.
fn loose_params(method: &str) -> Option<Value> {
    let id = |what: &str| json!({ "type": "integer", "required": true, "description": format!("The {what}'s id.") });
    let ai = || {
        json!({
            "tableId": id("table"),
            "fieldId": id("AI field"),
            "recordId": { "type": "integer", "description": "One record to run on." },
            "action": { "type": "string", "description": "The AI action (summarize, translate, extract, improve, customize, classify, text_generation, image_generation, image_customization); when given, it must be the field's own." },
            "config": { "type": "object", "additionalProperties": true, "description": "Overrides of the field's AI configuration for this run." },
        })
    };
    Some(match method {
        "ai.prepare" => {
            let mut params = ai();
            params["record_ids"] =
                json!({ "type": "array", "items": { "type": "integer" }, "description": "The records to run on, instead of recordId." });
            params["limit"] = json!({ "type": "integer", "description": "How many records to prepare at most." });
            params
        }
        "ai.apply" => {
            let mut params = ai();
            params["results"] = json!({
                "type": "array",
                "required": true,
                "description": "What the host generated, one entry per record.",
                "items": {
                    "type": "object",
                    "additionalProperties": true,
                    "properties": {
                        "recordId": { "type": "integer", "required": true },
                        "text": { "type": "string" },
                        "image": { "type": "json", "description": "A generated image." },
                        "images": { "type": "array", "items": { "type": "json" } },
                        "error": { "type": "string", "description": "Why generation failed for this record." },
                    },
                },
            });
            params
        }
        "changes.since" => json!({
            "version": { "type": "string", "description": "Base64 of a version a previous answer gave; empty for every change." },
        }),
        "read" => json!({
            "tableId": { "type": "integer", "description": "The table whose records to list, in order (a table's id, as inspect and table.tables.list answer it)." },
            "viewId": { "type": "integer", "description": "Instead of tableId: the view whose records to list, only those its filter keeps, in its order (a view's id, as inspect and table.views.list answer it)." },
            "limit": { "type": "integer", "description": "How many records to list, 1 to 200; default 50." },
            "cursor": { "type": "string", "description": "Where to go on: the cursor in a previous answer's next (the number of records before the page, counted from 0, as a string); default the first record." },
        }),
        "preview.show" => json!({
            "tableId": { "type": "integer", "description": "The table to show, with its first view unless viewId names another. Default: the first table." },
            "viewId": { "type": "integer", "description": "The view to show (its table is shown)." },
            "dashboardId": { "type": "string", "description": "A dashboard to show instead of a table." },
        }),
        _ => return None,
    })
}

/// The `ParameterSchemaSpec` of a command (A5), or `None` for a command this kit does not have.
pub fn input_schema(command: &str) -> Option<Value> {
    let mut schema = raw_input_schema(command)?;
    for (key, value) in schema.as_object_mut().into_iter().flatten() {
        if crate::params::kind(key).is_some() {
            crate::address::id_schema(value);
        } else {
            crate::address::schema(value);
        }
    }
    Some(schema)
}

fn raw_input_schema(command: &str) -> Option<Value> {
    if matches!(command, "edit.undo" | "edit.redo") {
        // Whose step (KitModel M12a, engine rule E14a): by default the handle's own scope.
        return Some(json!({ "scope": undo_scope_schema() }));
    }
    if command == crate::export::EXPORT_COMMAND {
        return Some(crate::export::input_schema());
    }
    if command == crate::preview::RENDER_COMMAND {
        return Some(crate::preview::input_schema());
    }
    if command == crate::outline::OUTLINE_COMMAND {
        return Some(crate::outline::input_schema());
    }
    let method = command.strip_prefix(COMMAND_PREFIX)?;
    let route = DOCUMENT_METHODS.iter().chain(METHODS).chain(ENGINE_METHODS).find(|(name, _)| *name == method).map(|(_, route)| *route)?;
    let mut params = Map::new();
    for id in route_ids(route) {
        let schema = match id {
            "dashboardId" => json!({ "type": "string", "required": true, "description": "The dashboard's id." }),
            "hash" => {
                json!({ "type": "string", "required": true, "description": "The blob's SHA-256, as an attachment's url names it after kition-blob:." })
            }
            other => json!({ "type": "integer", "required": true, "description": format!("The {}'s id.", other.trim_end_matches("Id")) }),
        };
        params.insert(id.to_owned(), schema);
    }
    for (name, schema) in query_params(method) {
        params.insert(name.to_owned(), schema);
    }
    if let Some(body) = body_schema(method) {
        let converted = from_json_schema(body.as_value(), &defs(&body), 0);
        if let Some(properties) = converted.get("properties").and_then(Value::as_object) {
            for (name, schema) in properties {
                params.entry(name.clone()).or_insert_with(|| schema.clone());
            }
        }
    }
    if let Some(Value::Object(loose)) = loose_params(method) {
        params.extend(loose);
    }
    if matches!(method, "undo" | "redo") {
        params.insert("scope".to_owned(), undo_scope_schema());
    }
    with_types(method, &mut params);
    if matches!(method, "records.create" | "records.update") {
        params["values"]["description"] = json!(
            "Values keyed by field name. Creation must supply every writable required field; inspect the table first. For the starter table, use {\"title\":\"New record\"}."
        );
        params["values"]["examples"] = json!([{"title":"New record"}]);
    }
    if !READ_METHODS.contains(&method) && !matches!(method, "undo" | "redo") {
        params.insert(
            AS_ORIGIN_PARAM.to_owned(),
            json!({ "type": "string", "description": "system:<name> writes as a host feature, outside the undo history. Agents leave it out." }),
        );
    }
    Some(Value::Object(params))
}

/// What each field type ([`FIELD_TYPES`]) holds, one line each, for the `type` of a new or
/// changed field.
const FIELD_TYPE_NOTES: &[(&str, &str)] = &[
    ("text", "One line of text (a string)."),
    ("long_text", "Text of several lines (a string); options.showAs shows it as text or rendered markdown."),
    ("number", "A number."),
    ("date", "A calendar date written as YYYY-MM-DD; options.date_format is how the editor shows it."),
    ("datetime", "A date and time as an ISO 8601 string; options.date_format is how the editor shows it."),
    ("single_select", "One of options.choices (a string)."),
    ("multi_select", "Some of options.choices (a list of strings)."),
    ("checkbox", "true or false."),
    ("url", "A web address (a string)."),
    ("attachment", "Files kept in the document: a list of the values table.attachments.put answers."),
    ("document_link", "A reference to another document (a string)."),
    ("formula", "Computed from the field's `formula` expression, which names fields in braces ({price} * 2); read-only."),
    ("ai", "Written by the host's model as options.ai_config says (table.ai.prepare, then table.ai.apply); read-only."),
    ("rating", "A whole number from 0 to options.max (default 5)."),
    ("user", "A person, {id, name, avatar?}, or a list of them."),
    ("link_to_record", "Links to records of the table options.target_table_id names; set with table.links.replace, read as [{row_key, display}]."),
    ("lookup", "Computed: the values of options.target_field_id in the records linked through options.link_field_id."),
    ("rollup", "Computed: options.aggregation over the records linked through options.link_field_id (their options.target_field_id values)."),
    ("button", "A button labelled options.label that runs options.action; it holds no value."),
    ("created_time", "Reserved for when the record was created; this engine leaves it empty."),
    ("last_modified_time", "Reserved for when the record last changed; this engine leaves it empty."),
    ("created_by", "Reserved for who created the record; this engine leaves it empty."),
    ("last_modified_by", "Reserved for who last changed the record; this engine leaves it empty."),
    ("auto_number", "Reserved for a running number; this engine leaves it empty."),
];

/// What each view type ([`VIEW_TYPES`]) shows, one line each.
const VIEW_TYPE_NOTES: &[(&str, &str)] = &[
    ("grid", "Rows and columns."),
    (
        "kanban",
        "Cards in columns, one per choice of a single_select field (the field of the config's first groups entry, else the first single_select) or per checkbox value.",
    ),
    ("calendar", "Records placed on the table's first date or datetime field."),
    ("gallery", "Cards with a cover picture (config cover_field_name, else the first attachment or url field)."),
];

/// A `type` parameter: an enum of `ids`, described one line per id from `notes`.
fn type_enum(heading: &str, ids: &[&str], notes: &[(&str, &str)]) -> Value {
    let lines: Vec<String> =
        ids.iter().map(|id| format!("{id}: {}", notes.iter().find(|(name, _)| name == id).map_or("", |(_, note)| *note))).collect();
    json!({ "type": "string", "enum": ids, "description": format!("{heading}\n{}", lines.join("\n")) })
}

fn field_type_spec() -> Value {
    type_enum(
        "The field's type (default text). There is no email or phone type: use text, or url for a link to open.",
        &FIELD_TYPES,
        FIELD_TYPE_NOTES,
    )
}

fn view_type_spec() -> Value {
    type_enum("The view's type (default grid).", &VIEW_TYPES, VIEW_TYPE_NOTES)
}

/// The options the field types read: each says which types read it. Other keys
/// (the editor's `column_tone`, `description`, `choice_tones`) are kept as they are.
fn field_options_spec() -> Value {
    let id = |text: &str| json!({ "type": "integer", "description": text });
    json!({
        "type": "object",
        "additionalProperties": true,
        "description": "Settings of the field's type: only the keys its type reads matter, and other keys are kept as they are.",
        "properties": {
            "choices": {
                "type": "array",
                "items": { "type": "string" },
                "description": "single_select, multi_select: the values to choose from, in order, for example [\"Lead\", \"Customer\"].",
            },
            "target_table_id": id("link_to_record: the id of the table whose records it links to."),
            "allow_multiple": { "type": "boolean", "description": "link_to_record: false allows one linked record per cell; default true." },
            "symmetric_field_id": id("link_to_record: the link field of the target table that links back, if any."),
            "link_field_id": id("lookup, rollup: the link_to_record field of this table to follow."),
            "target_field_id": id("lookup, rollup: the field of the linked records to read; a rollup without it aggregates the linked records themselves (count)."),
            "multi_value_mode": {
                "type": "string",
                "enum": ["list", "concat"],
                "description": "lookup: list answers the values as a list (default), concat as one string.",
            },
            "aggregation": {
                "type": "string",
                "enum": ["count", "sum", "avg", "max", "min", "and", "or", "concat"],
                "description": "rollup: how the linked values combine.",
            },
            "max": id("rating: the highest rating, 1 to 10; default 5."),
            "icon": { "type": "string", "enum": ["star", "heart", "thumbs"], "description": "rating: the symbol shown; default star." },
            "date_format": {
                "type": "string",
                "enum": [
                    "year_month_day_slash", "year_month_day_time_slash", "year_month_day_time_zone_slash", "year_month_day_dash",
                    "year_month_day_time_dash", "year_month_day_time_zone_dash", "month_day_dash", "month_day_year_slash", "day_month_year_slash",
                ],
                "description": "date, datetime: how the editor shows the value; the stored value does not change.",
            },
            "showAs": { "type": "string", "enum": ["text", "markdown"], "description": "long_text: show the value as plain text or as rendered markdown." },
            "label": { "type": "string", "description": "button: the button's text; default the field's title." },
            "action": {
                "type": "json",
                "description": "button: what it does, {\"kind\": \"open_url\", \"url\"} or {\"kind\": \"run_workflow\", \"workflow_id\"}.",
            },
            "ai_config": {
                "type": "object",
                "additionalProperties": true,
                "description": "ai (and text, long_text or attachment fields the host's model fills): what the model does, checked when the field runs. Also summary_instruction, extract_instruction or schema, improve_instruction, max_words, enabled, and image settings.",
                "properties": {
                    "type": {
                        "type": "string",
                        "enum": ["summarize", "translate", "extract", "improve", "customize", "classify", "text_generation", "image_generation", "image_customization"],
                        "description": "The action.",
                    },
                    "source_field_id": { "type": "integer", "description": "The field it reads; not needed by customize, text_generation and image_customization." },
                    "prompt": { "type": "string", "description": "The instruction of customize, text_generation and image_customization." },
                    "target_language": { "type": "string", "description": "translate: the language to write." },
                    "categories": { "type": "array", "items": { "type": "string" }, "description": "classify: two or more categories." },
                },
            },
        },
    })
}

/// The field and view `type` of the commands that take one, as enums of the engine's identifiers,
/// and the field `options` the types read.
fn with_types(method: &str, params: &mut Map<String, Value>) {
    let field = |params: &mut Map<String, Value>| {
        params.insert("type".to_owned(), field_type_spec());
        params.insert("options".to_owned(), field_options_spec());
    };
    let named = |params: &mut Map<String, Value>, of: &str| {
        params.insert("name".to_owned(), name_spec(of));
    };
    let items = |params: &mut Map<String, Value>, list: &str| -> Option<Map<String, Value>> {
        params.get_mut(list)?.get_mut("items")?.get_mut("properties")?.as_object_mut().map(std::mem::take)
    };
    match method {
        "tables.create" => {
            named(params, "table");
            for list in ["fields", "views"] {
                if let Some(mut properties) = items(params, list) {
                    if list == "fields" {
                        field(&mut properties);
                        named(&mut properties, "field");
                    } else {
                        properties.insert("type".to_owned(), view_type_spec());
                    }
                    params[list]["items"]["properties"] = Value::Object(properties);
                }
            }
        }
        "fields.create" => {
            field(params);
            named(params, "field");
        }
        "fields.update" => field(params),
        "views.create" | "views.update" => {
            params.insert("type".to_owned(), view_type_spec());
        }
        _ => {}
    }
}

/// A table's or field's `name`: kept as given, or the call is refused.
fn name_spec(of: &str) -> Value {
    let unique = if of == "table" { "among the document's tables" } else { "among the table's fields" };
    json!({
        "type": "string",
        "description": format!(
            "The {of}'s internal name, the key of its values when a field: lowercase ASCII letters, digits and `_`, starting with a letter and not ending with `_` (`^[a-z](?:[a-z0-9_]*[a-z0-9])?$`), unique {unique}. An explicit name is kept as given; one not in that form, or already taken (also by an earlier field of the same call), is refused with invalid_params. Without one it is derived from the title: letters and digits kept and lowercased, every other run of characters made one `_`, `_` trimmed, `f_` put before a leading digit, `field` when nothing is left (a title in Chinese, say, for a table too), then `_2`, `_3`, ... when taken."
        ),
    })
}

/// The `scope` parameter of `edit.undo` and `edit.redo`.
fn undo_scope_schema() -> Value {
    json!({
        "type": "string",
        "enum": ["person", "agent"],
        "description": "Whose step: the person's or the agent's. By default the scope of whoever runs it (an agent undoes only its own steps).",
    })
}

/// The ids a route template names (`{tableId}`), without the document's.
fn route_ids(route: &str) -> Vec<&str> {
    route.split('{').skip(1).filter_map(|rest| rest.split_once('}').map(|(id, _)| id)).filter(|id| *id != "documentId").collect()
}

fn defs(schema: &Schema) -> Map<String, Value> {
    schema.as_value().get("$defs").and_then(Value::as_object).cloned().unwrap_or_default()
}

/// A JSON Schema node (as `schemars` writes it) as a `ValueSchemaSpec` node: `$ref`s inlined
/// (a type inside itself up to [`MAX_DEPTH`] times), `Option`'s `null` branch dropped (the engine reads `null` as absent),
/// `const` as a one-value `enum`, tuples as arrays, any value as `json`, and every object with
/// an explicit `additionalProperties` (false for a struct, true for a map).
pub fn from_json_schema(node: &Value, defs: &Map<String, Value>, depth: usize) -> Value {
    let Some(object) = node.as_object() else { return json!({ "type": "json" }) };
    let description = object.get("description").cloned();
    let with_description = |mut value: Value| {
        if let (Some(text), Some(map)) = (&description, value.as_object_mut()) {
            map.entry("description").or_insert_with(|| text.clone());
        }
        value
    };
    if depth > MAX_DEPTH {
        return with_description(json!({ "type": "json" }));
    }
    if let Some(target) = object.get("$ref").and_then(Value::as_str) {
        let name = target.rsplit('/').next().unwrap_or_default();
        return with_description(defs.get(name).map_or_else(|| json!({ "type": "json" }), |def| from_json_schema(def, defs, depth + 1)));
    }
    for key in ["oneOf", "anyOf"] {
        if let Some(branches) = object.get(key).and_then(Value::as_array) {
            let mut kept: Vec<Value> = branches
                .iter()
                .filter(|branch| branch.get("type") != Some(&json!("null")))
                .map(|branch| from_json_schema(branch, defs, depth))
                .collect();
            let mut value = if kept.len() == 1 { kept.remove(0) } else { json!({ "oneOf": kept }) };
            if object.contains_key("properties") || object.contains_key("type") {
                // A struct with a variant part: keep it simple for models.
                value = json!({ "type": "json" });
            }
            return with_description(value);
        }
    }
    let kind = match object.get("type") {
        Some(Value::String(kind)) => kind.clone(),
        Some(Value::Array(kinds)) => kinds.iter().filter_map(Value::as_str).find(|kind| *kind != "null").unwrap_or("null").to_owned(),
        _ if object.contains_key("properties") => "object".to_owned(),
        _ => return with_description(json!({ "type": "json" })),
    };
    let mut out = Map::new();
    match kind.as_str() {
        "object" => {
            out.insert("type".into(), json!("object"));
            let required: Vec<&str> =
                object.get("required").and_then(Value::as_array).map(|all| all.iter().filter_map(Value::as_str).collect()).unwrap_or_default();
            if let Some(properties) = object.get("properties").and_then(Value::as_object) {
                let converted: Map<String, Value> = properties
                    .iter()
                    .map(|(name, inner)| {
                        let mut value = from_json_schema(inner, defs, depth);
                        if required.contains(&name.as_str())
                            && let Some(map) = value.as_object_mut()
                        {
                            map.insert("required".into(), Value::Bool(true));
                        }
                        (name.clone(), value)
                    })
                    .collect();
                out.insert("properties".into(), Value::Object(converted));
                out.insert(
                    "additionalProperties".into(),
                    Value::Bool(object.get("additionalProperties").is_some_and(|extra| extra != &Value::Bool(false))),
                );
            } else {
                out.insert("additionalProperties".into(), Value::Bool(object.get("additionalProperties") != Some(&Value::Bool(false))));
            }
        }
        "array" => {
            out.insert("type".into(), json!("array"));
            match object.get("items") {
                Some(items) if items.is_object() => {
                    out.insert("items".into(), from_json_schema(items, defs, depth));
                }
                _ => {
                    out.insert("items".into(), json!({ "type": "json" }));
                }
            }
        }
        "string" | "number" | "integer" | "boolean" | "null" => {
            out.insert("type".into(), json!(kind));
            if let Some(constant) = object.get("const") {
                out.insert("enum".into(), json!([constant]));
            } else if let Some(choices) = object.get("enum").and_then(Value::as_array) {
                let choices: Vec<Value> = choices.iter().filter(|choice| !choice.is_null()).cloned().collect();
                if !choices.is_empty() {
                    out.insert("enum".into(), Value::Array(choices));
                }
            }
        }
        _ => return with_description(json!({ "type": "json" })),
    }
    if let Some(default) = object.get("default").filter(|default| !default.is_null()) {
        out.insert("default".into(), default.clone());
    }
    with_description(Value::Object(out))
}

/// The A18 error a failed call answers.
fn error_spec() -> Value {
    json!({
        "type": "object",
        "additionalProperties": false,
        "description": "Why the call failed (A18); the file is as it was.",
        "properties": {
            "code": {
                "type": "string",
                "required": true,
                "enum": ["invalid-params", "invalid-op", "invalid-value", "not-found", "read-only", "unknown-command", "command-failed", "not-open", "draft-required", "not-allowed", "unsupported", "unavailable", "failed"],
            },
            "message": { "type": "string", "required": true },
            "index": { "type": "integer", "description": "The failing step or operation." },
            "retryable": { "type": "boolean" },
        },
    })
}

/// An open object with typed properties: every key the engine answers is named,
/// and a later key does not break a host that validates.
fn shape(description: &str, properties: Value) -> Value {
    json!({ "type": "object", "additionalProperties": true, "description": description, "properties": properties })
}

fn int(description: &str) -> Value {
    json!({ "type": "integer", "description": description })
}

fn text(description: &str) -> Value {
    json!({ "type": "string", "description": description })
}

fn flag(description: &str) -> Value {
    json!({ "type": "boolean", "description": description })
}

fn list(item: Value) -> Value {
    json!({ "type": "array", "items": item })
}

/// An object whose keys are the document's own (a record's values, a field's options).
fn map(description: &str) -> Value {
    json!({ "type": "object", "additionalProperties": true, "description": description })
}

fn any(description: &str) -> Value {
    json!({ "type": "json", "description": description })
}

fn or_null(node: Value) -> Value {
    let description = node.get("description").cloned();
    let mut value = json!({ "oneOf": [node, { "type": "null" }] });
    if let Some(description) = description {
        value["description"] = description;
    }
    value
}

/// `user_id`, `document_id`, `created_at` and `updated_at`, which every table, field, view and
/// record answers.
fn stamped(mut properties: Value) -> Value {
    properties["user_id"] = int("Always 0: the document's own user.");
    properties["document_id"] = int("Always 1: the document's id.");
    properties["created_at"] = int("When it was created, in Unix milliseconds.");
    properties["updated_at"] = int("When it last changed, in Unix milliseconds.");
    properties
}

fn table_shape() -> Value {
    shape(
        "A table with its fields and views.",
        stamped(json!({
            "id": int("The table's local projection address (tableId)."),
            "tool_id": text("The stable UUID accepted by tableId and used by receipts and conflicts."),
            "name": text("The table's internal name."),
            "title": text("The table's title."),
            "description": text("The table's description."),
            "order": int("Its position among the tables, from 1."),
            "primary_field_id": or_null(int("The id of the field that names a record.")),
            "meta": map("Metadata the host keeps with the table."),
            "fields": list(field_shape()),
            "views": list(view_shape()),
        })),
    )
}

fn field_shape() -> Value {
    let type_ids: Vec<&str> = FIELD_TYPES.to_vec();
    shape(
        "A field (column).",
        stamped(json!({
            "id": int("The field's local projection address (fieldId)."),
            "tool_id": text("The stable UUID accepted by fieldId and used by receipts and conflicts."),
            "table_id": int("The table's id."),
            "name": text("The key of its values in a record's `values`."),
            "title": text("The title people see."),
            "type": { "type": "string", "enum": type_ids, "description": "The field type." },
            "required": flag("A record needs a value."),
            "unique": flag("Values must differ between records."),
            "readonly": flag("Values are not written by hand (formula and AI fields)."),
            "is_primary": flag("It names the record."),
            "order": int("Its position among the table's fields, from 1."),
            "options": map("The settings its type reads (choices, target_table_id, …)."),
            "formula": text("A formula field's expression."),
            "ai_prompt": text("A legacy AI prompt."),
            "meta": map("Metadata the host keeps with the field."),
        })),
    )
}

fn view_shape() -> Value {
    let type_ids: Vec<&str> = VIEW_TYPES.to_vec();
    shape(
        "A view of a table.",
        stamped(json!({
            "id": int("The view's local projection address (viewId)."),
            "tool_id": text("The stable UUID accepted by viewId and used by receipts and conflicts."),
            "table_id": int("The table's id."),
            "title": text("The view's title."),
            "type": { "type": "string", "enum": type_ids, "description": "The view type." },
            "order": int("Its position among the table's views, from 1."),
            "locked": flag("The view is locked against changes in the editor."),
            "config": map("Its filters, sorts, groups and display settings."),
        })),
    )
}

fn record_shape() -> Value {
    shape(
        "A record (row).",
        stamped(json!({
            "id": int("The record's local projection address (recordId)."),
            "tool_id": text("The stable UUID accepted by recordId and used by receipts and conflicts."),
            "table_id": int("The table's id."),
            "row_key": text("The record's stable key, used by links, bulk commands and transact."),
            "order": int("Its position in the table, from 1."),
            "values": map("Its values, keyed by field name; a missing key is an empty value."),
        })),
    )
}

fn dashboard_shape() -> Value {
    shape(
        "A dashboard.",
        json!({
            "id": text("The dashboard's id."),
            "title": text("The dashboard's title."),
            "order": json!({ "type": "number", "description": "Its position among the dashboards." }),
            "source_table_id": any("The table its widgets read by default, or null."),
            "layout": list(any("A widget's position.")),
            "widgets": list(map("A widget (chart, number, list), with its own id.")),
        }),
    )
}

fn page(description: &str, item: Value) -> Value {
    shape(description, json!({ "total": int("How many there are."), "items": list(item) }))
}

fn message() -> Value {
    shape("What was done.", json!({ "message": text("What was deleted.") }))
}

fn link_items() -> Value {
    shape(
        "Records, as links show them.",
        json!({ "items": list(shape("A linked record.", json!({ "row_key": text("The record's row key."), "display": text("Its primary value as text.") }))) }),
    )
}

fn sort_items() -> Value {
    shape(
        "The fields, first to last.",
        json!({
            "items": list(shape("One field.", json!({
                "field_id": int("The field's id."),
                "direction": { "type": "string", "enum": ["asc", "desc"], "description": "Ascending or descending." },
            }))),
        }),
    )
}

/// A filter group: `{conjunction, children}`, each child a `{condition}` or a nested `{group}`.
fn filter_group(depth: usize) -> Value {
    let group = if depth == 0 { any("A nested group, as this one.") } else { filter_group(depth - 1) };
    shape(
        "A group of conditions.",
        json!({
            "conjunction": { "type": "string", "enum": ["and", "or"], "description": "Whether all children or any child must hold." },
            "children": list(shape("A condition or a nested group.", json!({
                "condition": shape("A field condition.", json!({
                    "field_id": int("The field's id."),
                    "operator": text("The comparison, such as eq, neq, contains, is_empty or is_not_empty."),
                    "value": any("The value to compare with."),
                })),
                "group": group,
            }))),
        }),
    )
}

fn view_field_shape() -> Value {
    shape(
        "One field's settings in one view.",
        json!({
            "view_id": int("The view's id."),
            "field_id": int("The field's id."),
            "visible": flag("Shown in the view."),
            "width": int("Its column width in pixels, 40 to 1200."),
            "position": int("Its position in the view."),
            "frozen": flag("Kept in view while scrolling sideways."),
        }),
    )
}

fn attachment_shape(description: &str, extra: Value) -> Value {
    let mut properties = json!({
        "name": text("The file's name."),
        "mimeType": text("The file's media type."),
        "sizeBytes": int("The file's size in bytes."),
    });
    if let (Some(properties), Value::Object(extra)) = (properties.as_object_mut(), extra) {
        properties.extend(extra);
    }
    shape(description, properties)
}

fn import_warning() -> Value {
    shape(
        "Something the import noticed.",
        json!({
            "code": text("A stable code."),
            "message": text("What happened."),
            "severity": text("info, warning or error."),
            "row": int("The row, from 1, when it is about one."),
            "column": int("The column, from 1, when it is about one."),
        }),
    )
}

fn import_preview() -> Value {
    shape(
        "What the file holds and how it would be imported.",
        json!({
            "import_token": text("The preview's token (sha256 of the file)."),
            "expires_at": int("When the preview expires; left out, since the kit keeps none."),
            "source": shape("Where the file came from.", json!({
                "kind": text("upload."), "workspace_path": text("Unused by the kit."), "upload_name": text("The file's name."),
            })),
            "filename": text("The file's name."),
            "format": text("csv, tsv, xlsx, xls or ods."),
            "encoding": text("The text encoding of a CSV or TSV file."),
            "delimiter": text("The delimiter of a CSV or TSV file."),
            "selected_sheet": text("The spreadsheet's sheet read."),
            "row_count": int("How many data rows."),
            "field_count": int("How many columns."),
            "fields": list(shape("A column and the field it would become.", json!({
                "index": int("The column, from 0."),
                "title": text("Its header."),
                "type": text("The field type detected."),
                "nullable": flag("Some rows leave it empty."),
                "options": map("The detected options (a select's choices)."),
                "sample_values": list(any("A value of the first rows.")),
            }))),
            "sample_rows": list(list(any("A cell."))),
            "warnings": list(import_warning()),
            "sheets": list(shape("A sheet of a spreadsheet.", json!({
                "name": text("Its name."), "index": int("Its position, from 0."), "hidden": flag("Hidden in the spreadsheet."),
                "row_count": int("How many data rows."), "field_count": int("How many columns."),
            }))),
        }),
    )
}

fn import_result() -> Value {
    shape(
        "What the import did.",
        json!({
            "document_id": int("The document's id."),
            "table_id": int("The table written."),
            "path": text("The document's path."),
            "rows_total": int("Rows read."),
            "rows_created": int("Records created."),
            "rows_updated": int("Records updated (upsert)."),
            "rows_skipped": int("Rows left out."),
            "fields_created": int("Fields created."),
            "fields_updated": int("Fields changed."),
            "warnings": list(import_warning()),
        }),
    )
}

fn ai_state() -> Value {
    shape(
        "The AI cell's run state.",
        json!({
            "status": text("done."),
            "source_hash": text("A hash of what the model read."),
            "config_hash": text("A hash of the field's AI configuration."),
            "model_id": int("Always 0."),
            "updated_at": int("When it was written, in Unix milliseconds."),
            "error": text("Empty, or why it failed."),
        }),
    )
}

fn record_change() -> Value {
    shape(
        "One record a write touched.",
        json!({
            "tableId": int("The table's id."),
            "recordId": int("The record's id."),
            "fieldIds": list(int("A field whose value changed (updates only).")),
            "origin": text("Who wrote it: kind:principal, or a system origin's label."),
        }),
    )
}

/// What a `TableChange` looks like where answers carry it (`kition_table_api::respond::change_json`):
/// the type's own fields plus `documentId` and `origin`.
fn change_spec() -> Value {
    let schema = schema_for!(kition_table_file::TableChange);
    let mut change = from_json_schema(schema.as_value(), &defs(&schema), 0);
    if let Some(properties) = change.get_mut("properties").and_then(Value::as_object_mut) {
        properties.insert("documentId".into(), json!({ "type": "string" }));
        properties.insert("origin".into(), json!({ "type": "string", "description": "Who wrote it: kind:principal, or a system origin's label." }));
        for list in ["recordsUpserted", "recordsDeleted"] {
            properties.insert(
                list.into(),
                json!({ "type": "array", "description": "[tableId, rowKey] pairs.", "items": { "type": "array", "items": { "type": "json" } } }),
            );
        }
    }
    change
}

/// The `result` of a method: the route's answer, every key named.
fn result_spec(method: &str) -> Value {
    match method {
        "documents.get" => shape(
            "The document with its tables and dashboards.",
            json!({
                "id": int("Always 1."),
                "tool_id": text("The document's stable Base UUID, used by queries, batches and receipts."),
                "user_id": int("Always 0."),
                "workspace_root": text("Always empty."),
                "path": text("The document's name as the host opened it."),
                "title": text("The document's title, else its name without .ktable."),
                "description": text("Always empty."),
                "icon": text("Always empty."),
                "color": text("Always empty."),
                "meta": any("Always null."),
                "created_at": int("When the file was created, in Unix milliseconds."),
                "updated_at": int("When it last changed, in Unix milliseconds."),
                "tables": list(table_shape()),
                "dashboards": list(shape("A dashboard's summary.", json!({
                    "id": text("The dashboard's id."), "title": text("Its title."), "order": json!({ "type": "number", "description": "Its position." }),
                }))),
            }),
        ),
        "tables.list" => page("Every table, with its fields and views.", table_shape()),
        "tables.create" | "tables.update" => table_shape(),
        "fields.list" => page("The table's fields, in order.", field_shape()),
        "fields.create" | "fields.update" | "fields.duplicate" => field_shape(),
        "views.list" => page("The table's views, in order.", view_shape()),
        "views.create" | "views.update" => view_shape(),
        "tables.delete" | "fields.delete" | "views.delete" | "records.delete" => message(),
        "views.getFilterTree" | "views.putFilterTree" => shape("The view's filter.", json!({ "tree": or_null(filter_group(1)) })),
        "views.getSorts" | "views.putSorts" | "views.getGroups" | "views.putGroups" => sort_items(),
        "views.patchConfig" => shape(
            "The view's whole configuration after the change.",
            json!({ "config": map("filter_tree, sorts, groups, row_height, frozen_column_count, and the editor's settings.") }),
        ),
        "views.listFields" => {
            shape("The settings the view stores; fields without any show with the defaults.", json!({ "items": list(view_field_shape()) }))
        }
        "views.patchField" => view_field_shape(),
        "records.list" => shape(
            "A page of records.",
            json!({
                "items": list(record_shape()),
                "total": int("How many records the table has."),
                "offset": int("How many were skipped."),
                "limit": int("The page size."),
            }),
        ),
        "records.create" | "records.update" => {
            let mut record = record_shape();
            if let Some(properties) = record.get_mut("properties").and_then(Value::as_object_mut) {
                properties.insert(
                    "changed".into(),
                    json!({ "type": "boolean", "description": "Whether the call changed the record: false when every value was already so." }),
                );
            }
            record
        }
        "records.history" => shape(
            "A page of the record's history, newest first.",
            json!({
                "items": list(shape("One change.", json!({
                    "id": int("The entry's id."),
                    "document_id": text("The document's KDM id."),
                    "table_id": int("The table's id."),
                    "record_id": int("The record's id."),
                    "field_id": or_null(int("The field changed; null for a create or delete.")),
                    "action": { "type": "string", "enum": ["create", "update", "delete"], "description": "What happened." },
                    "before": any("The value before (the whole record's values for a delete)."),
                    "after": any("The value after (the whole record's values for a create)."),
                    "actor": text("Who made the change."),
                    "created_at": int("When, in Unix milliseconds."),
                }))),
                "total": int("How many entries there are."),
                "limit": int("The page size."),
                "offset": int("How many were skipped."),
            }),
        ),
        "records.bulkDelete" => shape(
            "What was deleted.",
            json!({ "deleted_count": int("How many records were deleted."), "missing_keys": list(text("A row key that names no record.")) }),
        ),
        "records.bulkCopy" => shape(
            "The copies.",
            json!({ "copied_count": int("How many records were copied."), "new_row_keys": list(text("A copy's row key, in order.")) }),
        ),
        "links.list" | "links.replace" | "fields.linkCandidates" => link_items(),
        "links.delete" => shape("Done.", json!({ "ok": flag("Always true.") })),
        "csv.import" => shape("What was added.", json!({ "created": int("How many records were created.") })),
        "csv.export" => shape("The table as CSV.", json!({ "name": text("A file name, the table's title."), "content": text("The CSV text.") })),
        "markdown.export" => shape("The table as Markdown.", json!({ "content": text("The Markdown table.") })),
        "transact" => shape(
            "What the operations did.",
            json!({ "change": change_spec(), "created": list(text("The row key of a record the operations created.")) }),
        ),
        "undo" | "redo" => {
            shape("Whether a step was taken back (or put back).", json!({ "done": flag("False when there was no step."), "change": change_spec() }))
        }
        "historyState" => {
            shape("What Undo and Redo would do.", json!({ "canUndo": flag("There is a step to undo."), "canRedo": flag("There is a step to redo.") }))
        }
        "dashboards.list" => shape("Every dashboard.", json!({ "items": list(dashboard_shape()), "total": int("How many there are.") })),
        "dashboards.upsert" => dashboard_shape(),
        "dashboards.delete" => shape("Done.", json!({ "deleted": flag("Always true.") })),
        "attachments.put" => attachment_shape(
            "The attachment value: put it in the list an attachment field's value holds.",
            json!({ "url": text("kition-blob: and the bytes' SHA-256."), "sha256": text("The bytes' SHA-256.") }),
        ),
        "attachments.read" => {
            attachment_shape("The attachment with its bytes.", json!({ "hash": text("The bytes' SHA-256."), "data": text("The bytes, base64.") }))
        }
        "import.preview" => import_preview(),
        "import.run" => import_result(),
        "ai.prepare" => shape(
            "The model requests of the run, for the host to generate and answer with table.ai.apply.",
            json!({
                "fieldId": int("The AI field's id."),
                "output": { "type": "string", "enum": ["text", "image"], "description": "What the model makes." },
                "requests": list(shape("One record's request.", json!({
                    "recordId": int("The record's id."),
                    "messages": list(shape("A message to the model.", json!({
                        "role": text("user."),
                        "parts": list(map("A part: {type: text, text} or {type: image, mime, hash?, url?}.")),
                    }))),
                    "options": map("An image request's count, size, width, height, quality and useCase."),
                }))),
                "skipped": list(shape("A record left out of a batch.", json!({ "recordId": int("The record's id."), "reason": text("Why.") }))),
                "model": text("The model the field asks for, when it names one."),
            }),
        ),
        "ai.apply" => shape(
            "What was written: one record's `record`, `field` and `state`, or a batch's `items` and counts.",
            json!({
                "record": record_shape(),
                "field": field_shape(),
                "state": ai_state(),
                "items": list(shape("A record written.", json!({ "record": record_shape(), "state": ai_state() }))),
                "accepted": int("Results taken (a batch)."),
                "completed": int("Records written."),
                "failed": int("Results that failed."),
                "errors": list(shape("A result that failed.", json!({ "record_id": int("The record's id."), "message": text("Why.") }))),
                "limit": int("The most records one batch takes."),
            }),
        ),
        "changes.since" => shape(
            "The records written since the version.",
            json!({
                "version": text("The current version, base64: the next call's `version`."),
                "created": list(record_change()),
                "updated": list(record_change()),
                "deleted": list(record_change()),
            }),
        ),
        "read" => shape(
            "A page of the records of a table or a view.",
            json!({
                "tableId": int("The table's id."),
                "viewId": int("The view's id, when a view was read."),
                "columns": list(text("A field's name: the key of its values in table.records.create and table.records.update.")),
                "rows": list(shape("A record.", json!({
                    "id": int("The record's local projection address (recordId)."),
                    "tool_id": text("The stable UUID accepted by recordId and used by receipts and conflicts."),
                    "cells": list(any("Its value in the column at the same position; null when empty. A link's records are {id, display}, by record id; text longer than 200 characters is cut and ends in an ellipsis.")),
                }))),
                "from": int("The first listed record's position, counted from 1."),
                "to": int("The last listed record's position, counted from 1."),
                "total": int("How many records the table has, or the view shows."),
                "next": shape("The step that lists the records after these; absent after the last.", json!({
                    "command": text("table.read."),
                    "params": map("Its params: the same tableId or viewId, and the cursor to go on from."),
                })),
            }),
        ),
        "preview.show" => shape(
            "What the picture shows now.",
            json!({
                "tableId": or_null(int("The table shown; null for a dashboard.")),
                "table": text("The table's title."),
                "viewId": or_null(int("The view shown.")),
                "view": text("The view's title."),
                "viewType": text("The view's type: grid, kanban, calendar or gallery."),
                "dashboardId": or_null(text("The dashboard shown; null for a table.")),
                "dashboard": text("The dashboard's title."),
                "records": int("How many records the view shows."),
            }),
        ),
        other => json!({ "type": "json", "description": format!("The answer of {other}.") }),
    }
}

/// The `ValueSchemaSpec` of a command's answer (A1): `{ result, change }`, or `{ error }` (A18).
/// Nothing is required, so both validate.
pub fn output_schema(command: &str) -> Option<Value> {
    if command == crate::export::EXPORT_COMMAND {
        // The file at the top level, where the host reads it (`agent.export`).
        return Some(crate::export::output_schema(error_spec()));
    }
    if command == crate::preview::RENDER_COMMAND {
        // The PNG at the top level, where the kits server reads it (`agent.render`, K21).
        return Some(crate::preview::output_schema(error_spec()));
    }
    if command == crate::outline::OUTLINE_COMMAND {
        // The outline at the top level, where the host reads it (`outline.command`, K24).
        return Some(crate::outline::output_schema(error_spec()));
    }
    let method = match command {
        "edit.undo" => "undo",
        "edit.redo" => "redo",
        other => other.strip_prefix(COMMAND_PREFIX).filter(|method| crate::known_method(method))?,
    };
    Some(json!({
        "type": "object",
        "additionalProperties": false,
        "description": "The request's own answer is in `result`, beside `change`: what the call changed in the document (null when nothing changed). A failed call answers `error` instead (A18).",
        "properties": {
            "result": result_spec(method),
            "change": { "oneOf": [change_spec(), { "type": "null" }], "description": "What the call changed, or null." },
            "error": error_spec(),
        },
    }))
}

/// A `ValueSchemaSpec` node as JSON Schema, the way the headless host and the MCP binding convert
/// it: `json` has no type, and `required` moves from each property to its parent's list.
pub fn to_json_schema(spec: &Value) -> Value {
    let Value::Object(map) = spec else { return spec.clone() };
    let mut out = Map::new();
    for (key, value) in map {
        match key.as_str() {
            "type" if value == "json" => {}
            "required" if value.is_boolean() => {}
            "items" => {
                out.insert(key.clone(), to_json_schema(value));
            }
            "oneOf" => {
                out.insert(key.clone(), Value::Array(value.as_array().map(|all| all.iter().map(to_json_schema).collect()).unwrap_or_default()));
            }
            "properties" => {
                let mut required = Vec::new();
                let properties = value
                    .as_object()
                    .map(|all| {
                        all.iter()
                            .map(|(name, inner)| {
                                if inner.get("required") == Some(&Value::Bool(true)) {
                                    required.push(Value::String(name.clone()));
                                }
                                (name.clone(), to_json_schema(inner))
                            })
                            .collect()
                    })
                    .unwrap_or_default();
                out.insert(key.clone(), Value::Object(properties));
                if !required.is_empty() {
                    out.insert("required".into(), Value::Array(required));
                }
            }
            _ => {
                out.insert(key.clone(), value.clone());
            }
        }
    }
    Value::Object(out)
}

/// A command's parameters as the JSON Schema of `inputSchema`.
pub fn input_json_schema(command: &str) -> Option<Value> {
    input_schema(command).map(|params| to_json_schema(&json!({ "type": "object", "additionalProperties": false, "properties": params })))
}

/// Every agent-facing command with its tool name (A2) and both JSON Schemas: the contents of
/// `agent/commands.json`, which a test keeps equal to this.
pub fn commands_json(commands: &[kition_kit::KitCommand]) -> Value {
    let list: Vec<Value> = commands
        .iter()
        .filter_map(|command| {
            Some(json!({
                "command": command.id,
                "tool": format!("{TOOL_PREFIX}_{}", command.id.replace(|c: char| !c.is_ascii_alphanumeric() && c != '_', "_")),
                "title": command.title,
                "description": command.description,
                "mutates": command.mutates,
                "inputSchema": input_json_schema(&command.id)?,
                "outputSchema": to_json_schema(&output_schema(&command.id)?),
            }))
        })
        .collect();
    json!({ "khp": "1.0", "kit": "kit.table", "commands": list })
}
