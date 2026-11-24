//! Tables, fields, views, records, and attachments.

use kition_kdm::ObjectId;
use kition_time::{Millis, now_millis};
use rusqlite::{Connection, OptionalExtension, Row, params};
use serde::Serialize;
use serde_json::{Map, Value};

use crate::json::{self, encode_default, fallback};
use crate::kdm::cells::Texts;
use crate::kdm::commit::{
    LinkFields, any_object, by_field_id, field_id_of, field_object, field_objects_by_name, link_records, record_id_of, record_object, split_links,
    table_id_of, table_object, view_id_of, view_object,
};
use crate::kdm::model;
use crate::kdm::options::{self, IdKind};
use crate::{Result, TableFile, TableFileError};

pub const FIELD_TYPES: [&str; 24] = [
    "text",
    "long_text",
    "number",
    "date",
    "datetime",
    "single_select",
    "multi_select",
    "checkbox",
    "url",
    "attachment",
    "document_link",
    "formula",
    "ai",
    "rating",
    "user",
    "link_to_record",
    "lookup",
    "rollup",
    "button",
    "created_time",
    "last_modified_time",
    "created_by",
    "last_modified_by",
    "auto_number",
];

pub const VIEW_TYPES: [&str; 4] = ["grid", "kanban", "calendar", "gallery"];

#[derive(Debug, Clone, Default)]
pub struct TableSpec {
    pub name: String,
    pub title: String,
    pub description: String,
    pub fields: Vec<FieldSpec>,
    pub views: Vec<ViewSpec>,
    pub meta: Value,
}

#[derive(Debug, Clone, Default)]
pub struct FieldSpec {
    pub name: String,
    pub title: String,
    pub field_type: String,
    pub primary: bool,
    pub required: bool,
    pub readonly: bool,
    pub options: Value,
    pub formula: String,
    pub ai_prompt: String,
    pub meta: Value,
    /// Inserts at this position, shifting later fields; `None` appends.
    pub order: Option<i64>,
}

#[derive(Debug, Clone, Default)]
pub struct ViewSpec {
    pub title: String,
    pub view_type: String,
    pub config: Value,
}

#[derive(Debug, Clone, Default)]
pub struct TablePatch {
    pub title: Option<String>,
    pub description: Option<String>,
    /// `Some` replaces the metadata (with `Null` meaning `{}`).
    pub meta: Option<Value>,
    pub primary_field_id: Option<i64>,
}

#[derive(Debug, Clone, Default)]
pub struct FieldPatch {
    pub title: Option<String>,
    pub field_type: Option<String>,
    pub required: Option<bool>,
    pub readonly: Option<bool>,
    pub is_primary: Option<bool>,
    pub options: Option<Value>,
    pub formula: Option<String>,
    pub ai_prompt: Option<String>,
    pub meta: Option<Value>,
}

#[derive(Debug, Clone, Default)]
pub struct ViewPatch {
    pub title: Option<String>,
    pub view_type: Option<String>,
    pub config: Option<Value>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Table {
    pub id: i64,
    pub document_id: String,
    pub name: String,
    pub title: String,
    pub description: String,
    pub order: i64,
    pub primary_field_id: Option<i64>,
    pub meta: Map<String, Value>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub fields: Vec<Field>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub views: Vec<View>,
    pub created_at: Millis,
    pub updated_at: Millis,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct Field {
    pub id: i64,
    pub document_id: String,
    pub table_id: i64,
    pub name: String,
    pub title: String,
    #[serde(rename = "type")]
    pub field_type: String,
    pub required: bool,
    pub unique: bool,
    pub readonly: bool,
    pub is_primary: bool,
    pub order: i64,
    pub options: Map<String, Value>,
    pub formula: String,
    pub ai_prompt: String,
    pub meta: Map<String, Value>,
    pub created_at: Millis,
    pub updated_at: Millis,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct View {
    pub id: i64,
    pub document_id: String,
    pub table_id: i64,
    pub title: String,
    #[serde(rename = "type")]
    pub view_type: String,
    pub order: i64,
    pub locked: bool,
    pub config: Map<String, Value>,
    pub created_at: Millis,
    pub updated_at: Millis,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Record {
    pub id: i64,
    pub document_id: String,
    pub table_id: i64,
    pub row_key: String,
    pub order: i64,
    pub values: Map<String, Value>,
    pub created_at: Millis,
    pub updated_at: Millis,
}

#[derive(Debug, Clone, Default)]
pub struct AttachmentSpec {
    pub document_id: String,
    pub record_id: Option<i64>,
    pub field_id: Option<i64>,
    pub name: String,
    pub path: String,
    pub mime_type: String,
    pub size: i64,
    pub meta: Value,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Attachment {
    pub id: i64,
    pub document_id: String,
    pub record_id: Option<i64>,
    pub field_id: Option<i64>,
    pub name: String,
    pub path: String,
    pub mime_type: String,
    pub size: i64,
    pub meta: Map<String, Value>,
    pub created_at: Millis,
}

const TABLE_COLUMNS: &str = "id, document_id, name, title, description, position, primary_field_id, meta_json, created_at, updated_at";
pub(crate) const FIELD_COLUMNS: &str = "id, document_id, table_id, name, title, type, required, unique_value, readonly, \
    is_primary, position, options_json, formula, ai_prompt, meta_json, created_at, updated_at";
const VIEW_COLUMNS: &str = "id, document_id, table_id, title, type, position, locked, config_json, created_at, updated_at";
const RECORD_COLUMNS: &str = "id, document_id, table_id, row_key, position, values_json, created_at, updated_at";

fn table_from_row(row: &Row<'_>) -> rusqlite::Result<Table> {
    Ok(Table {
        id: row.get(0)?,
        document_id: row.get(1)?,
        name: row.get(2)?,
        title: row.get(3)?,
        description: row.get(4)?,
        order: row.get(5)?,
        primary_field_id: row.get(6)?,
        meta: json::object(&row.get::<_, String>(7)?),
        fields: Vec::new(),
        views: Vec::new(),
        created_at: row.get(8)?,
        updated_at: row.get(9)?,
    })
}

pub(crate) fn field_from_row(row: &Row<'_>) -> rusqlite::Result<Field> {
    Ok(Field {
        id: row.get(0)?,
        document_id: row.get(1)?,
        table_id: row.get(2)?,
        name: row.get(3)?,
        title: row.get(4)?,
        field_type: row.get(5)?,
        required: row.get::<_, i64>(6)? != 0,
        unique: row.get::<_, i64>(7)? != 0,
        readonly: row.get::<_, i64>(8)? != 0,
        is_primary: row.get::<_, i64>(9)? != 0,
        order: row.get(10)?,
        options: json::object(&row.get::<_, String>(11)?),
        formula: row.get(12)?,
        ai_prompt: row.get(13)?,
        meta: json::object(&row.get::<_, String>(14)?),
        created_at: row.get(15)?,
        updated_at: row.get(16)?,
    })
}

fn view_from_row(row: &Row<'_>) -> rusqlite::Result<View> {
    Ok(View {
        id: row.get(0)?,
        document_id: row.get(1)?,
        table_id: row.get(2)?,
        title: row.get(3)?,
        view_type: row.get(4)?,
        order: row.get(5)?,
        locked: row.get::<_, i64>(6)? != 0,
        config: json::object(&row.get::<_, String>(7)?),
        created_at: row.get(8)?,
        updated_at: row.get(9)?,
    })
}

pub(crate) fn record_from_row(row: &Row<'_>) -> rusqlite::Result<Record> {
    Ok(Record {
        id: row.get(0)?,
        document_id: row.get(1)?,
        table_id: row.get(2)?,
        row_key: row.get(3)?,
        order: row.get(4)?,
        values: json::object(&row.get::<_, String>(5)?),
        created_at: row.get(6)?,
        updated_at: row.get(7)?,
    })
}

impl TableFile {
    pub fn kind(&self) -> &'static str {
        "ktable"
    }

    pub fn document_id(&self) -> Result<String> {
        self.get_meta("document_id")
    }

    pub fn list_tables(&self) -> Result<Vec<Table>> {
        let mut statement =
            self.conn.prepare(&format!("SELECT {TABLE_COLUMNS} FROM kition_tables WHERE deleted_at IS NULL ORDER BY position ASC, id ASC"))?;
        let mut tables = statement.query_map([], table_from_row)?.collect::<std::result::Result<Vec<_>, _>>()?;
        for table in &mut tables {
            self.fill_table_schema(table)?;
        }
        Ok(tables)
    }

    pub fn get_table(&self, table_id: i64) -> Result<Table> {
        let mut table = self.conn.query_row(
            &format!("SELECT {TABLE_COLUMNS} FROM kition_tables WHERE id = ?1 AND deleted_at IS NULL"),
            [table_id],
            table_from_row,
        )?;
        self.fill_table_schema(&mut table)?;
        Ok(table)
    }

    fn fill_table_schema(&self, table: &mut Table) -> Result<()> {
        table.fields = self.list_fields(table.id)?;
        table.views = self.list_views(table.id)?;
        Ok(())
    }

    pub fn list_fields(&self, table_id: i64) -> Result<Vec<Field>> {
        let mut statement = self.conn.prepare(&format!(
            "SELECT {FIELD_COLUMNS} FROM kition_fields WHERE table_id = ?1 AND deleted_at IS NULL ORDER BY position ASC, id ASC"
        ))?;
        Ok(statement.query_map([table_id], field_from_row)?.collect::<std::result::Result<Vec<_>, _>>()?)
    }

    fn list_views(&self, table_id: i64) -> Result<Vec<View>> {
        let mut statement = self
            .conn
            .prepare(&format!("SELECT {VIEW_COLUMNS} FROM kition_views WHERE table_id = ?1 AND deleted_at IS NULL ORDER BY position ASC, id ASC"))?;
        Ok(statement.query_map([table_id], view_from_row)?.collect::<std::result::Result<Vec<_>, _>>()?)
    }

    pub fn create_table(&mut self, spec: &TableSpec) -> Result<Table> {
        self.writable()?;
        let base = self.require_base()?;
        let spec = TableSpec { fields: spec.fields.iter().map(|field| self.field_spec_in(field)).collect::<Result<Vec<_>>>()?, ..spec.clone() };
        let (ids, _) = self.commit(|tx| model::create_table(tx, &base, &spec, None))?;
        let table_id = table_id_of(&self.conn, &ids.table)?;
        self.get_table(table_id)
    }

    pub fn update_table(&mut self, table_id: i64, patch: &TablePatch) -> Result<Table> {
        self.writable()?;
        let table = table_object(&self.conn, table_id)?;
        let primary = match patch.primary_field_id {
            Some(field_id) => Some(field_object(&self.conn, table_id, field_id)?),
            None => None,
        };
        let patch = TablePatch { primary_field_id: None, ..patch.clone() };
        self.commit(|tx| model::update_table(tx, &table, &patch, primary.as_ref()))?;
        self.get_table(table_id)
    }

    pub fn delete_table(&mut self, table_id: i64) -> Result<()> {
        self.writable()?;
        let base = self.require_base()?;
        let table = table_object(&self.conn, table_id)?;
        self.commit(|tx| model::delete_table(tx, &base, &table))?;
        Ok(())
    }

    pub fn create_field(&mut self, table_id: i64, spec: &FieldSpec) -> Result<Field> {
        self.writable()?;
        let table = table_object(&self.conn, table_id)?;
        let spec = self.field_spec_in(spec)?;
        let (field, _) = self.commit(|tx| model::create_field(tx, &table, &spec))?;
        let field_id = field_id_of(&self.conn, &field)?;
        self.get_field(table_id, field_id)
    }

    /// Copies a field's settings into a new non-primary field placed right
    /// after it, titled "<title> copy".
    pub fn duplicate_field(&mut self, table_id: i64, field_id: i64) -> Result<Field> {
        self.writable()?;
        let source = self.get_field(table_id, field_id)?;
        let spec = FieldSpec {
            name: String::new(),
            title: format!("{} copy", source.title),
            field_type: source.field_type.clone(),
            primary: false,
            required: source.required,
            readonly: source.readonly,
            options: Value::Object(source.options.clone()),
            formula: source.formula.clone(),
            ai_prompt: source.ai_prompt.clone(),
            meta: Value::Object(source.meta.clone()),
            order: Some(self.field_index(table_id, field_id)? + 2),
        };
        self.create_field(table_id, &spec)
    }

    pub fn update_field(&mut self, table_id: i64, field_id: i64, patch: &FieldPatch) -> Result<Field> {
        self.writable()?;
        let field = field_object(&self.conn, table_id, field_id)?;
        let patch = FieldPatch {
            options: match &patch.options {
                Some(options) => Some(options_in(&self.conn, options)?),
                None => None,
            },
            ..patch.clone()
        };
        // What the field's texts read now: its prompt, and the long texts of its cells when its
        // type changes (they are written again in the new type's shape).
        let mut texts = if patch.field_type.is_some() {
            let table = table_object(&self.conn, table_id)?;
            Texts::of_records(&self.replica, &self.model_view().record_ids(&table)?)
        } else {
            Texts::default()
        };
        texts.read(&self.replica, &field, "aiPrompt");
        self.commit(|tx| model::update_field_known(tx, &field, &patch, &texts))?;
        self.get_field(table_id, field_id)
    }

    pub fn delete_field(&mut self, table_id: i64, field_id: i64) -> Result<()> {
        self.writable()?;
        let table = table_object(&self.conn, table_id)?;
        let field = field_object(&self.conn, table_id, field_id)?;
        self.commit(|tx| model::delete_field(tx, &table, &field))?;
        Ok(())
    }

    pub fn create_view(&mut self, table_id: i64, spec: &ViewSpec) -> Result<View> {
        self.writable()?;
        let table = table_object(&self.conn, table_id)?;
        let spec = ViewSpec { config: self.config_in(table_id, &spec.config)?, ..spec.clone() };
        let (view, _) = self.commit(|tx| model::create_view(tx, &table, &spec))?;
        let view_id = view_id_of(&self.conn, &view)?;
        self.get_view(table_id, view_id)
    }

    /// Patches a view. A new `config` replaces the old one, except that its sorts and groups
    /// (which [`TableFile::replace_view_sorts`] and [`TableFile::replace_view_groups`] write too)
    /// are kept when the new config does not name them.
    pub fn update_view(&mut self, table_id: i64, view_id: i64, patch: &ViewPatch) -> Result<View> {
        self.writable()?;
        let view = view_object(&self.conn, table_id, view_id)?;
        let current = self.get_view(table_id, view_id)?;
        let config = match &patch.config {
            Some(config) => Some(self.config_in(table_id, &crate::view_config::keep_orderings(&current.config, config)?)?),
            None => None,
        };
        let patch = ViewPatch { config, ..patch.clone() };
        self.commit(|tx| model::update_view(tx, &view, &patch))?;
        self.get_view(table_id, view_id)
    }

    pub fn delete_view(&mut self, table_id: i64, view_id: i64) -> Result<()> {
        self.writable()?;
        let table = table_object(&self.conn, table_id)?;
        let view = view_object(&self.conn, table_id, view_id)?;
        self.commit(|tx| model::delete_view(tx, &table, &view))?;
        Ok(())
    }

    /// A view config of the table as the API writes it, with every field it names only by
    /// integer id (`field_id`) named by name too, so the model stores the field's object id
    /// ([`crate::kdm::view_refs`]). A JSON string is decoded first.
    fn config_in(&self, table_id: i64, config: &Value) -> Result<Value> {
        let config = match config {
            Value::String(raw) if raw.trim().is_empty() => Value::Null,
            Value::String(raw) => serde_json::from_str(raw).map_err(|_| TableFileError::invalid("invalid json string"))?,
            other => other.clone(),
        };
        let names: std::collections::HashMap<i64, String> = self.list_fields(table_id)?.into_iter().map(|field| (field.id, field.name)).collect();
        Ok(crate::kdm::view_refs::name_local_ids(&config, &|id| names.get(&id).cloned()))
    }

    /// The document's base, or [`TableFileError::Invalid`] for a replica without one.
    pub(crate) fn require_base(&self) -> Result<ObjectId> {
        self.base.clone().ok_or_else(|| TableFileError::invalid("the table document has no base"))
    }

    /// A field spec with its options in the document's form (object ids).
    fn field_spec_in(&self, spec: &FieldSpec) -> Result<FieldSpec> {
        Ok(FieldSpec { options: options_in(&self.conn, &spec.options)?, ..spec.clone() })
    }

    /// The 0-based index of a live field among its table's live fields, in order.
    fn field_index(&self, table_id: i64, field_id: i64) -> Result<i64> {
        let fields = self.list_fields(table_id)?;
        let index = fields.iter().position(|field| field.id == field_id).ok_or(TableFileError::NotFound)?;
        Ok(i64::try_from(index).unwrap_or(i64::MAX))
    }

    pub fn list_records(&self, table_id: i64) -> Result<Vec<Record>> {
        self.list_records_page(table_id, 0, 0)
    }

    /// The live records of a table in order, each holding only the values of the fields `names`
    /// (the cells an arrangement reads), for the fields with a value. Reads a table of 100,000
    /// records without parsing every cell of it. A name holding `"` is read from the whole value.
    pub fn list_record_values(&self, table_id: i64, names: &[String]) -> Result<Vec<Record>> {
        self.record_values(table_id, names, None)
    }

    /// [`TableFile::list_record_values`] of the records `keep` takes: it is handed each live
    /// record's values as the JSON text the file holds (an object of the fields' values by name),
    /// in order, and the records it refuses are never parsed.
    pub fn list_record_values_where(&self, table_id: i64, names: &[String], keep: &mut dyn FnMut(&str) -> bool) -> Result<Vec<Record>> {
        self.record_values(table_id, names, Some(keep))
    }

    fn record_values(&self, table_id: i64, names: &[String], mut keep: Option<&mut dyn FnMut(&str) -> bool>) -> Result<Vec<Record>> {
        let paths: Vec<String> = names.iter().filter(|name| !name.contains('"')).map(|name| format!("$.\"{name}\"")).collect();
        if paths.len() != names.len() {
            let mut records = self.list_records(table_id)?;
            if let Some(keep) = keep.as_mut() {
                records.retain(|record| keep(&Value::Object(record.values.clone()).to_string()));
            }
            for record in &mut records {
                record.values.retain(|name, _| names.contains(name));
            }
            return Ok(records);
        }
        // The whole value only when `keep` reads it: the cells come from SQLite's JSON paths.
        let mut columns = String::from("id, document_id, table_id, row_key, position, created_at, updated_at");
        let first = if keep.is_some() {
            columns.push_str(", values_json");
            8
        } else {
            7
        };
        for index in 0..paths.len() {
            columns.push_str(&format!(", values_json -> ?{}", index + 2));
        }
        let mut statement = self
            .conn
            .prepare(&format!("SELECT {columns} FROM kition_records WHERE table_id = ?1 AND deleted_at IS NULL ORDER BY position ASC, id ASC"))?;
        let mut binds: Vec<&dyn rusqlite::ToSql> = vec![&table_id];
        binds.extend(paths.iter().map(|path| path as &dyn rusqlite::ToSql));
        let mut rows = statement.query(binds.as_slice())?;
        let mut records = Vec::new();
        while let Some(row) = rows.next()? {
            if let Some(keep) = keep.as_mut() {
                let whole = match row.get_ref(7)? {
                    rusqlite::types::ValueRef::Text(text) => std::str::from_utf8(text).unwrap_or_default(),
                    _ => "",
                };
                if !keep(whole) {
                    continue;
                }
            }
            let mut values = Map::new();
            for (index, name) in names.iter().enumerate() {
                if let Some(text) = row.get::<_, Option<String>>(index + first)? {
                    values.insert(name.clone(), serde_json::from_str(&text).unwrap_or(Value::Null));
                }
            }
            records.push(Record {
                id: row.get(0)?,
                document_id: row.get(1)?,
                table_id: row.get(2)?,
                row_key: row.get(3)?,
                order: row.get(4)?,
                values,
                created_at: row.get(5)?,
                updated_at: row.get(6)?,
            });
        }
        Ok(records)
    }

    /// The live records of a table among `row_keys`, whole, in order; a key of no live record of
    /// the table (deleted, or never there) is left out.
    pub fn records_by_row_keys(&self, table_id: i64, row_keys: &[String]) -> Result<Vec<Record>> {
        let mut statement =
            self.conn.prepare(&format!("SELECT {RECORD_COLUMNS} FROM kition_records WHERE table_id = ?1 AND row_key = ?2 AND deleted_at IS NULL"))?;
        let mut records = Vec::new();
        for key in row_keys {
            if let Some(record) = statement.query_row(params![table_id, key], record_from_row).optional()? {
                records.push(record);
            }
        }
        records.sort_by_key(|record| (record.order, record.id));
        Ok(records)
    }

    pub fn count_records(&self, table_id: i64) -> Result<i64> {
        Ok(self.conn.query_row("SELECT COUNT(*) FROM kition_records WHERE table_id = ?1 AND deleted_at IS NULL", [table_id], |row| row.get(0))?)
    }

    /// With `limit > 0`, returns up to `limit` records whose position is
    /// greater than `offset` (keyset pagination on position).
    pub fn list_records_page(&self, table_id: i64, limit: i64, offset: i64) -> Result<Vec<Record>> {
        let base = format!("SELECT {RECORD_COLUMNS} FROM kition_records WHERE table_id = ?1 AND deleted_at IS NULL");
        let records = if limit > 0 {
            let mut statement = self.conn.prepare(&format!("{base} AND position > ?2 ORDER BY position ASC, id ASC LIMIT ?3"))?;
            statement.query_map(params![table_id, offset.max(0), limit], record_from_row)?.collect::<std::result::Result<Vec<_>, _>>()?
        } else {
            let mut statement = self.conn.prepare(&format!("{base} ORDER BY position ASC, id ASC"))?;
            statement.query_map([table_id], record_from_row)?.collect::<std::result::Result<Vec<_>, _>>()?
        };
        Ok(records)
    }

    pub fn get_record(&self, table_id: i64, record_id: i64) -> Result<Record> {
        Ok(self.conn.query_row(
            &format!("SELECT {RECORD_COLUMNS} FROM kition_records WHERE id = ?1 AND table_id = ?2 AND deleted_at IS NULL"),
            params![record_id, table_id],
            record_from_row,
        )?)
    }

    pub fn create_record(&mut self, table_id: i64, values: &Map<String, Value>) -> Result<Record> {
        self.create_records(table_id, std::slice::from_ref(values))?.pop().ok_or(TableFileError::NotFound)
    }

    /// Creates records at the end of the table, in order, as one transaction (one change row,
    /// one undo step): much faster than one [`TableFile::create_record`] per row for imports.
    /// Every row is checked like `create_record` before anything is written.
    pub fn create_records(&mut self, table_id: i64, rows: &[Map<String, Value>]) -> Result<Vec<Record>> {
        self.writable()?;
        let table = self.get_table(table_id)?;
        let names = field_objects_by_name(&self.conn, table_id)?;
        let link_fields = LinkFields::read(&self.conn, table_id)?;
        let mut cells = Vec::with_capacity(rows.len());
        let mut links = Vec::with_capacity(rows.len());
        for values in rows {
            let (row, link_cells) = link_fields.split(&self.conn, &validate_record_values(&table.fields, values)?)?;
            cells.push(by_field_id(&names, &row));
            links.push(link_cells);
        }
        let table_object = table_object(&self.conn, table_id)?;
        let created_by = self.actor();
        let (ids, _) = self.commit(|tx| {
            let ids = model::create_records(tx, &table_object, None, &cells, now_millis(), &created_by)?;
            link_records(tx, &ids, &links)?;
            Ok(ids)
        })?;
        ids.iter()
            .map(|key| {
                let record_id = record_id_of(&self.conn, key.as_str())?;
                self.get_record(table_id, record_id)
            })
            .collect()
    }

    /// Writes the given cells of a record (other cells are left as they are; `null` clears);
    /// `order > 0` moves the record to just before the first other record at that position or
    /// later (the end when there is none). Required fields are checked on the merged values.
    pub fn update_record(&mut self, table_id: i64, record_id: i64, values: &Map<String, Value>, order: Option<i64>) -> Result<Record> {
        self.writable()?;
        let table = self.get_table(table_id)?;
        let record = self.get_record(table_id, record_id)?;
        let mut next = record.values.clone();
        for (key, value) in values {
            next.insert(key.clone(), value.clone());
        }
        validate_record_values(&table.fields, &next)?;
        let table_object = table_object(&self.conn, table_id)?;
        let record_object = ObjectId::from_string(record.row_key.clone());
        let (cells, links) = split_links(&self.conn, table_id, values)?;
        let cells = by_field_id(&field_objects_by_name(&self.conn, table_id)?, &cells);
        let moved = match order.filter(|order| *order > 0) {
            Some(order) => Some(
                self.conn
                    .query_row(
                        "SELECT row_key FROM kition_records WHERE table_id = ?1 AND deleted_at IS NULL \
                         AND id != ?2 AND position >= ?3 ORDER BY position ASC, id ASC LIMIT 1",
                        params![table_id, record_id, order],
                        |row| row.get::<_, String>(0),
                    )
                    .optional()?
                    .map(ObjectId::from_string),
            ),
            None => None,
        };
        let texts = Texts::of_records(&self.replica, [&record_object]);
        self.commit(|tx| {
            model::set_cells_known(tx, &table_object, &[(record_object.clone(), cells.clone())], &texts)?;
            link_records(tx, std::slice::from_ref(&record_object), std::slice::from_ref(&links))?;
            if let Some(before) = &moved {
                model::move_records(tx, &table_object, std::slice::from_ref(&record_object), before.as_ref())?;
            }
            Ok(())
        })?;
        self.get_record(table_id, record_id)
    }

    pub fn delete_record(&mut self, table_id: i64, record_id: i64) -> Result<()> {
        self.writable()?;
        let table = table_object(&self.conn, table_id)?;
        let record = record_object(&self.conn, table_id, record_id)?;
        let (deleted, _) = self.commit(|tx| model::delete_records(tx, &table, std::slice::from_ref(&record)))?;
        if deleted == 0 {
            return Err(TableFileError::NotFound);
        }
        Ok(())
    }

    /// Records an attachment. Attachments are file bookkeeping outside the table document (the
    /// KDM has no attachment objects), so this is a plain SQL write, not a KDM transaction.
    pub fn add_attachment(&self, spec: &AttachmentSpec) -> Result<Attachment> {
        self.writable()?;
        let document_id = match spec.document_id.trim() {
            "" => self.document_id()?,
            id => id.to_owned(),
        };
        let name = fallback(&spec.name, "attachment");
        let path = spec.path.trim().to_owned();
        if path.is_empty() {
            return Err(TableFileError::invalid("attachment path is required"));
        }
        let meta_json = encode_default(&spec.meta, "{}")?;
        let mime_type = spec.mime_type.trim().to_owned();
        let now = now_millis();
        self.conn.execute(
            "INSERT INTO kition_attachments (document_id, record_id, field_id, name, path, mime_type, size, meta_json, created_at) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
            params![document_id, spec.record_id, spec.field_id, name, path, mime_type, spec.size, meta_json, now],
        )?;
        Ok(Attachment {
            id: self.conn.last_insert_rowid(),
            document_id,
            record_id: spec.record_id,
            field_id: spec.field_id,
            name,
            path,
            mime_type,
            size: spec.size,
            meta: json::object(&meta_json),
            created_at: now,
        })
    }

    pub fn get_field(&self, table_id: i64, field_id: i64) -> Result<Field> {
        field_in(&self.conn, table_id, field_id)
    }

    pub fn get_view(&self, table_id: i64, view_id: i64) -> Result<View> {
        Ok(self.conn.query_row(
            &format!("SELECT {VIEW_COLUMNS} FROM kition_views WHERE id = ?1 AND table_id = ?2 AND deleted_at IS NULL"),
            params![view_id, table_id],
            view_from_row,
        )?)
    }
}

/// A live field of a table.
pub(crate) fn field_in(conn: &Connection, table_id: i64, field_id: i64) -> Result<Field> {
    Ok(conn.query_row(
        &format!("SELECT {FIELD_COLUMNS} FROM kition_fields WHERE id = ?1 AND table_id = ?2 AND deleted_at IS NULL"),
        params![field_id, table_id],
        field_from_row,
    )?)
}

/// A live field of any table.
pub(crate) fn field_by_id(conn: &Connection, field_id: i64) -> Result<Field> {
    let table_id: i64 = conn.query_row("SELECT table_id FROM kition_fields WHERE id = ?1 AND deleted_at IS NULL", [field_id], |row| row.get(0))?;
    field_in(conn, table_id, field_id)
}

/// Drops values for unknown fields and rejects blank required fields.
pub(crate) fn validate_record_values(fields: &[Field], values: &Map<String, Value>) -> Result<Map<String, Value>> {
    let clean: Map<String, Value> =
        values.iter().filter(|(key, _)| fields.iter().any(|field| &field.name == *key)).map(|(key, value)| (key.clone(), value.clone())).collect();
    for field in fields {
        if field.required && json::is_blank(clean.get(&field.name)) {
            return Err(TableFileError::invalid(format!("required field is blank: {}", field.title)));
        }
    }
    Ok(clean)
}

/// Looks up a live row ID or reports `NotFound`.
pub(crate) fn exists(conn: &Connection, sql: &str, id: i64) -> Result<()> {
    conn.query_row(sql, [id], |row| row.get::<_, i64>(0)).optional()?.ok_or(TableFileError::NotFound)?;
    Ok(())
}

/// Options from the relational API in the document's form: integer table and field ids become
/// object ids. An id naming no row the replica knows becomes `null` (a dangling reference, as a
/// deleted table or field leaves behind); a value that is no id is [`TableFileError::Invalid`].
pub(crate) fn options_in(conn: &Connection, options: &Value) -> Result<Value> {
    let Value::Object(map) = options else {
        return Ok(options.clone());
    };
    let mut out = map.clone();
    for id_key in options::ID_KEYS {
        let target = match id_key.parent {
            None => Some(&mut out),
            Some(parent) => out.get_mut(parent).and_then(Value::as_object_mut),
        };
        let Some(target) = target else {
            continue;
        };
        let Some(raw) = target.get(id_key.relational) else {
            continue;
        };
        let Some(id) = options::local_id(raw)? else {
            continue;
        };
        if lookup_object(conn, id_key.kind, id)?.is_none() {
            target.insert(id_key.relational.to_owned(), Value::Null);
        }
    }
    let resolved: std::cell::RefCell<Option<TableFileError>> = std::cell::RefCell::new(None);
    let value = options::options_to_kdm(&Value::Object(out), |kind, id| match lookup_object(conn, kind, id) {
        Ok(found) => found,
        Err(err) => {
            resolved.replace(Some(err));
            None
        }
    });
    if let Some(err) = resolved.take() {
        return Err(err);
    }
    value
}

fn lookup_object(conn: &Connection, kind: IdKind, id: i64) -> Result<Option<ObjectId>> {
    match kind {
        IdKind::Table => any_object(conn, "kition_tables", id),
        IdKind::Field => any_object(conn, "kition_fields", id),
    }
}
