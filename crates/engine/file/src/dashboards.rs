//! Dashboards: unlike tables, not relational (see `kdm::project`'s module doc) — read and
//! written straight through the KDM document. A dashboard's id is a KDM object id (a string),
//! not a small integer like a table's.

use kition_kdm::ObjectId;
use serde_json::{Map, Value};

use crate::kdm::commit::{any_object, table_id_of};
use crate::kdm::model;
use crate::kdm::options::{self, IdKind};
use crate::kdm::widgets;
use crate::{Result, TableFile, TableFileError};

/// One dashboard, in the relational API's shape: `config`'s table reference (when present) is
/// a local `source_table_id`, not a KDM object id (see
/// [`options::options_to_kdm`]/[`options::options_from_kdm`]). `widgets` are keyed by widget id.
#[derive(Debug, Clone, PartialEq)]
pub struct Dashboard {
    pub id: String,
    pub name: String,
    pub config: Value,
    pub widgets: Map<String, Value>,
}

impl TableFile {
    /// The base's dashboards in order.
    pub fn list_dashboards(&self) -> Result<Vec<Dashboard>> {
        let view = self.model_view();
        view.dashboards()?
            .into_iter()
            .map(|data| {
                let names = self.field_names(&data.config, |field| field.id.to_string(), |field| field.name.clone());
                let widgets = data
                    .widgets
                    .into_iter()
                    .map(|(key, mut widget)| {
                        if let Some(query) = widget.get_mut("query") {
                            *query = widgets::query_from_kdm(query, &|id| names.get(id.as_str()).cloned());
                        }
                        (key, widget)
                    })
                    .collect();
                Ok(Dashboard {
                    id: data.id.to_string(),
                    name: data.name,
                    config: options::options_from_kdm(&data.config, |object| table_id_of(&self.conn, object).ok()),
                    widgets,
                })
            })
            .collect()
    }

    /// The fields of a dashboard's table (its `source_table`, by object id), as `key` to `value`.
    fn field_names(
        &self,
        config: &Value,
        key: impl Fn(&crate::kdm::read::FieldData) -> String,
        value: impl Fn(&crate::kdm::read::FieldData) -> String,
    ) -> std::collections::HashMap<String, String> {
        let Some(table) = config.get("source_table").and_then(Value::as_str).map(ObjectId::from_string) else {
            return std::collections::HashMap::new();
        };
        self.model_view().fields(&table).unwrap_or_default().iter().map(|field| (key(field), value(field))).collect()
    }

    fn get_dashboard(&self, id: &str) -> Result<Dashboard> {
        self.list_dashboards()?.into_iter().find(|dashboard| dashboard.id == id).ok_or(TableFileError::NotFound)
    }

    /// `config` in the document's form: its relational table (and field, though no current
    /// caller's config has one) references resolved to KDM object ids.
    fn dashboard_config_in(&self, config: &Value) -> Result<Value> {
        options::options_to_kdm(config, |kind, local| match kind {
            IdKind::Table => any_object(&self.conn, "kition_tables", local).ok().flatten(),
            IdKind::Field => any_object(&self.conn, "kition_fields", local).ok().flatten(),
        })
    }

    /// Creates or updates a dashboard; `id: None` creates one with a new id, matching
    /// [`model::upsert_dashboard`]. `config` and widget values are in the relational API's
    /// shape (see [`Dashboard`]).
    pub fn upsert_dashboard(&mut self, id: Option<&str>, name: &str, config: Value, widgets: &Map<String, Value>) -> Result<Dashboard> {
        self.writable()?;
        let base = self.require_base()?;
        let kdm_config = self.dashboard_config_in(&config)?;
        let widgets = self.widgets_in(&kdm_config, widgets);
        let object_id = id.map(ObjectId::from_string);
        let (dashboard_id, _) = self.commit(|tx| model::upsert_dashboard(tx, &base, object_id.as_ref(), name, kdm_config, &widgets))?;
        self.get_dashboard(dashboard_id.as_str())
    }

    /// Widgets in the document's form: their queries name the dashboard table's fields by object
    /// id ([`widgets::query_to_kdm`]).
    fn widgets_in(&self, config: &Value, widgets: &Map<String, Value>) -> Map<String, Value> {
        let ids = self.field_names(config, |field| field.name.clone(), |field| field.id.to_string());
        widgets
            .iter()
            .map(|(key, widget)| {
                let mut widget = widget.clone();
                if let Some(query) = widget.get_mut("query") {
                    *query = widgets::query_to_kdm(query, &|name| ids.get(name).map(ObjectId::from_string));
                }
                (key.clone(), widget)
            })
            .collect()
    }

    pub fn delete_dashboard(&mut self, id: &str) -> Result<()> {
        self.writable()?;
        let base = self.require_base()?;
        let object_id = ObjectId::from_string(id);
        self.commit(|tx| model::delete_dashboard(tx, &base, &object_id))?;
        Ok(())
    }

    /// Moves dashboards (typically the workspace-DB's legacy `meta.dashboards`) into the file
    /// as KDM objects, keeping each one's id, as one transaction.
    pub fn import_dashboards(&mut self, dashboards: &[(String, String, Value, Map<String, Value>)]) -> Result<()> {
        self.writable()?;
        let base = self.require_base()?;
        let converted = dashboards
            .iter()
            .map(|(id, name, config, widgets)| {
                let config = self.dashboard_config_in(config)?;
                let widgets = self.widgets_in(&config, widgets);
                Ok((ObjectId::from_string(id.clone()), name.clone(), config, widgets))
            })
            .collect::<Result<Vec<_>>>()?;
        self.commit(|tx| {
            for (id, name, config, widgets) in &converted {
                model::upsert_dashboard(tx, &base, Some(id), name, config.clone(), widgets)?;
            }
            Ok(())
        })?;
        Ok(())
    }
}
