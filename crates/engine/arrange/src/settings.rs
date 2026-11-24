//! What a view does to its table's records: its filter, its groups and its sorts.
//!
//! A view's settings are its config's `filter_tree`, `sorts` and `groups` (the view's `Filter`,
//! `Sort` and `Group` objects), which the editor and the engine (`views.putFilterTree`,
//! `views.putSorts`, `views.putGroups`) write alike.

use kition_table_file::{Field, FilterTree, Record, TableFile, View, ViewSort};
use serde_json::{Map, Value};

use crate::filter::{Filter, count_conditions, tree_of_config};
use crate::rows::{Order, Row};

fn failed(error: impl std::fmt::Display) -> String {
    error.to_string()
}

/// One sort or group: its field by name or by id, and its direction.
struct Item {
    name: Option<String>,
    id: i64,
    descending: bool,
}

impl From<&ViewSort> for Item {
    fn from(sort: &ViewSort) -> Self {
        Self { name: None, id: sort.field_id, descending: sort.direction == "desc" }
    }
}

/// The sorts or groups as a config holds them (`[{field_name | field_id, direction}]`).
fn engine_items(config: &Map<String, Value>, key: &str) -> Option<Vec<Item>> {
    Some(
        config
            .get(key)?
            .as_array()?
            .iter()
            .map(|item| Item {
                name: item.get("field_name").and_then(Value::as_str).map(str::to_owned),
                id: item.get("field_id").and_then(Value::as_i64).unwrap_or(0),
                descending: item.get("direction").and_then(Value::as_str) == Some("desc"),
            })
            .collect(),
    )
}

/// What a view does to its table's records, as the editor applies it: its filter, then its
/// groups and sorts as one ordering (groups first). Each field comes with whether it runs
/// descending.
#[derive(Clone, Debug, Default)]
pub struct Arrangement {
    pub filter: Option<FilterTree>,
    pub groups: Vec<(Field, bool)>,
    pub sorts: Vec<(Field, bool)>,
}

impl Arrangement {
    /// The settings of `view` of a table with `fields`, as the view stores them; none without a
    /// view.
    pub fn of(file: &TableFile, fields: &[Field], view: Option<&View>) -> Result<Self, String> {
        match view {
            Some(view) => Self::of_config(file, fields, view, &view.config),
            None => Ok(Self::default()),
        }
    }

    /// The settings of `view` with the keys of `config` in place of the stored ones: what the
    /// editor shows while its settings are not saved yet (or cannot be, read-only). The sorts and
    /// groups stay the stored ones unless `config` names its own, as a config written without
    /// them keeps them (`views.update`).
    pub fn of_config(file: &TableFile, fields: &[Field], view: &View, config: &Map<String, Value>) -> Result<Self, String> {
        let stored = &view.config;
        let filter = match config.get("filter_tree") {
            Some(tree) => tree_of_config(tree, fields),
            // A view stores its filter under `filter_tree`; only a file written before it has rows.
            None if !stored.contains_key("filter_tree") => file.load_filter_tree(view.id).map_err(failed)?,
            None => None,
        };
        let engine = |key: &str, rows: &dyn Fn() -> Result<Vec<ViewSort>, String>| -> Result<Vec<(&Field, bool)>, String> {
            let items = match engine_items(config, key).or_else(|| engine_items(stored, key)) {
                Some(items) => items,
                None => rows()?.iter().map(Item::from).collect(),
            };
            Ok(items
                .iter()
                .filter_map(|item| {
                    let by_name = item.name.as_ref().and_then(|name| fields.iter().find(|field| &field.name == name));
                    let field = by_name.or_else(|| fields.iter().find(|field| field.id == item.id))?;
                    Some((field, item.descending))
                })
                .collect())
        };
        let groups = engine("groups", &|| file.list_view_groups(view.id).map_err(failed))?;
        let sorts = engine("sorts", &|| file.list_view_sorts(view.id).map_err(failed))?;
        let owned = |items: Vec<(&Field, bool)>| items.into_iter().map(|(field, descending)| (field.clone(), descending)).collect();
        Ok(Self { filter, groups: owned(groups), sorts: owned(sorts) })
    }

    /// How many filter conditions the view has.
    pub fn filters(&self) -> usize {
        self.filter.as_ref().map_or(0, count_conditions)
    }

    /// The filter, ready to test records of a table with `fields`.
    pub fn compiled_filter(&self, fields: &[Field]) -> Option<Filter> {
        self.filter.as_ref().map(|tree| Filter::new(tree, fields))
    }

    /// The order of the records: the groups' fields, then the sorts'.
    pub fn order(&self) -> Order {
        Order::new(self.groups.iter().chain(&self.sorts).map(|(field, descending)| (field.name.clone(), *descending)).collect())
    }

    /// `records` as the view shows them: the ones its filter keeps, in its order. Records that
    /// tie keep the table's order (their position, then their id).
    pub fn apply(&self, fields: &[Field], records: Vec<Record>) -> Vec<Record> {
        let filter = self.compiled_filter(fields);
        let order = self.order();
        let mut kept: Vec<(Row, Record)> = records
            .into_iter()
            .filter(|record| filter.as_ref().is_none_or(|filter| filter.keeps(&record.values)))
            .map(|record| (order.row(&record, &[]), record))
            .collect();
        kept.sort_by(|(a, _), (b, _)| order.cmp(a, b));
        kept.into_iter().map(|(_, record)| record).collect()
    }
}

/// The records of table `table_id` as its view `view_id` shows them (filtered and in its order),
/// or every record in order without a view; all of them. A view that is not the table's is an
/// error.
pub fn view_records(file: &TableFile, table_id: i64, view_id: Option<i64>) -> Result<Vec<Record>, String> {
    let table = file.get_table(table_id).map_err(|_| format!("not found: table {table_id}"))?;
    let records = file.list_records(table_id).map_err(failed)?;
    let view = match view_id {
        Some(id) => Some(table.views.iter().find(|view| view.id == id).ok_or_else(|| format!("not found: view {id} of table {table_id}"))?),
        None => None,
    };
    Ok(Arrangement::of(file, &table.fields, view)?.apply(&table.fields, records))
}
