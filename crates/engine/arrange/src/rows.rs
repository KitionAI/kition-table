//! The order of a view's records: each record as a row with its sort keys, compared field by
//! field, ties broken by the table's order.

use std::cmp::Ordering;

use kition_table_file::Record;
use serde_json::Value;

use crate::value::{display, natural_cmp, sort_key};

/// A shown record as the arrangement keeps it.
#[derive(Clone, Debug, PartialEq)]
pub struct Row {
    pub id: i64,
    /// Its position in the table (the order without sorts).
    pub position: i64,
    /// Its sort key for each field of the [`Order`].
    keys: Vec<Box<str>>,
    /// Its values of the fields the view groups by, outermost first.
    pub groups: Vec<Value>,
    /// The fields whose text holds the search, by id.
    pub hits: Vec<i64>,
}

/// The fields records are ordered by (groups first, then sorts), each with whether it runs
/// descending. Two rows equal on every field keep the table's order: position, then id. So the
/// order is total, and placing one row among the others gives what sorting them all gives.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Order {
    fields: Vec<(String, bool)>,
}

impl Order {
    pub fn new(fields: Vec<(String, bool)>) -> Self {
        Self { fields }
    }

    /// The names of the fields it reads.
    pub fn names(&self) -> impl Iterator<Item = &str> {
        self.fields.iter().map(|(name, _)| name.as_str())
    }

    /// `record` as a row: its keys, its values of `groups`, and its search `hits`.
    pub fn row(&self, record: &Record, groups: &[String], hits: Vec<i64>) -> Row {
        Row {
            id: record.id,
            position: record.order,
            keys: self.fields.iter().map(|(name, _)| sort_key(&display(record.values.get(name)))).collect(),
            groups: groups.iter().map(|name| record.values.get(name).cloned().unwrap_or(Value::Null)).collect(),
            hits,
        }
    }

    /// The order of two rows.
    pub fn cmp(&self, a: &Row, b: &Row) -> Ordering {
        for ((_, descending), (x, y)) in self.fields.iter().zip(a.keys.iter().zip(&b.keys)) {
            let order = natural_cmp(x, y);
            let order = if *descending { order.reverse() } else { order };
            if order != Ordering::Equal {
                return order;
            }
        }
        a.position.cmp(&b.position).then(a.id.cmp(&b.id))
    }
}
