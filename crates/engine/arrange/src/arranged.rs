//! A view's records arranged and kept arranged: the shown records in order, built once from the
//! table, then moved record by record as edits change them.
//!
//! A whole arrangement reads only the cells it needs (the filter's, the groups' and the sorts'
//! fields; every field while a search is on). After an edit only the records it touched are
//! taken out and put back where the order places them (a binary search: the order is total, so
//! this is where sorting everything again would put them). A change to the view, the fields or
//! the records' positions, or more touched records than [`RELOCATE_LIMIT`], arranges again.

use std::collections::{BTreeSet, HashMap, HashSet};

use kition_table_file::{Field, Record, TableFile};
use serde_json::Value;

use crate::axis::{Axis, GroupHeader};
use crate::filter::Filter;
use crate::rows::{Order, Row};
use crate::settings::Arrangement;
use crate::value::{display, js_json, same_value};

/// More touched records than this arrange the view again instead of moving them one by one.
pub const RELOCATE_LIMIT: usize = 512;

fn failed(error: impl std::fmt::Display) -> String {
    error.to_string()
}

/// The editor's group id of a header: its value and its parents', `g<depth>:<JSON>` joined with
/// `::` (the editor keeps which groups are collapsed by these ids).
pub fn group_id(values: &[Value]) -> String {
    values.iter().enumerate().map(|(depth, value)| format!("g{depth}:{}", js_json(Some(value)))).collect::<Vec<_>>().join("::")
}

/// The records of one table as a view and a search show them.
#[derive(Clone, Debug)]
pub struct Arranged {
    table_id: i64,
    /// Every field, `(id, name)`: the search reads them all.
    fields: Vec<(i64, String)>,
    filter: Option<Filter>,
    order: Order,
    /// The names of the fields the view groups by, outermost first.
    groups: Vec<String>,
    /// The search, lower-cased; empty for none.
    needle: String,
    /// The fields a whole arrangement reads (all of them while searching).
    reads: Option<Vec<String>>,
    rows: Vec<Row>,
    /// The row key of each shown record, by its id, and back.
    keys: HashMap<String, i64>,
    total: usize,
}

impl Arranged {
    /// Arranges the records of `table_id` (with `fields`) as `arrangement` and the search `query`
    /// (any field's text holding it, case ignored; blank for none) show them.
    pub fn new(file: &TableFile, table_id: i64, fields: &[Field], arrangement: &Arrangement, query: &str) -> Result<Self, String> {
        let filter = arrangement.compiled_filter(fields);
        let order = arrangement.order();
        let groups: Vec<String> = arrangement.groups.iter().map(|(field, _)| field.name.clone()).collect();
        let needle = query.trim().to_lowercase();
        let reads = needle.is_empty().then(|| {
            let mut names: Vec<String> = Vec::new();
            for name in order.names() {
                if !names.iter().any(|known| known == name) {
                    names.push(name.to_owned());
                }
            }
            if let Some(filter) = &filter {
                filter.fields(&mut names);
            }
            names
        });
        let mut arranged = Self {
            table_id,
            fields: fields.iter().map(|field| (field.id, field.name.clone())).collect(),
            filter,
            order,
            groups,
            needle,
            reads,
            rows: Vec::new(),
            keys: HashMap::new(),
            total: 0,
        };
        arranged.rebuild(file)?;
        Ok(arranged)
    }

    /// Arranges every record again.
    pub fn rebuild(&mut self, file: &TableFile) -> Result<(), String> {
        let records = match &self.reads {
            Some(names) => file.list_record_values(self.table_id, names).map_err(failed)?,
            None => file.list_records(self.table_id).map_err(failed)?,
        };
        self.total = records.len();
        self.keys.clear();
        let mut rows = Vec::with_capacity(records.len());
        for record in &records {
            if let Some(row) = self.row(record) {
                self.keys.insert(record.row_key.clone(), record.id);
                rows.push(row);
            }
        }
        rows.sort_by(|a, b| self.order.cmp(a, b));
        self.rows = rows;
        Ok(())
    }

    /// `record` as a row, when the view and the search show it.
    fn row(&self, record: &Record) -> Option<Row> {
        if self.filter.as_ref().is_some_and(|filter| !filter.keeps(&record.values)) {
            return None;
        }
        let hits: Vec<i64> = if self.needle.is_empty() {
            Vec::new()
        } else {
            let hits: Vec<i64> = self
                .fields
                .iter()
                .filter(|(_, name)| display(record.values.get(name)).to_lowercase().contains(&self.needle))
                .map(|(id, _)| *id)
                .collect();
            if hits.is_empty() {
                return None;
            }
            hits
        };
        Some(self.order.row(record, &self.groups, hits))
    }

    /// Moves the records with `row_keys` (created, changed or deleted since the arrangement) to
    /// where they now belong: out when deleted or no longer shown, in when newly shown. Answers
    /// whether the axis changed. More than [`RELOCATE_LIMIT`] keys arrange again.
    pub fn relocate(&mut self, file: &TableFile, row_keys: &BTreeSet<String>) -> Result<bool, String> {
        if row_keys.is_empty() {
            return Ok(false);
        }
        if row_keys.len() > RELOCATE_LIMIT {
            let before = self.axis(0, &BTreeSet::new());
            self.rebuild(file)?;
            return Ok(self.axis(0, &BTreeSet::new()) != before);
        }
        let total = self.total;
        let keys: Vec<String> = row_keys.iter().cloned().collect();
        let records = file.records_by_row_keys(self.table_id, &keys).map_err(failed)?;
        let gone: HashSet<i64> = keys.iter().filter_map(|key| self.keys.remove(key)).collect();
        let before: Vec<(usize, Row)> =
            self.rows.iter().enumerate().filter(|(_, row)| gone.contains(&row.id)).map(|(at, row)| (at, row.clone())).collect();
        if !gone.is_empty() {
            self.rows.retain(|row| !gone.contains(&row.id));
        }
        let mut placed: Vec<i64> = Vec::new();
        for record in &records {
            if let Some(row) = self.row(record) {
                let at = self.rows.partition_point(|other| self.order.cmp(other, &row).is_lt());
                self.rows.insert(at, row);
                self.keys.insert(record.row_key.clone(), record.id);
                placed.push(record.id);
            }
        }
        self.total = usize::try_from(file.count_records(self.table_id).map_err(failed)?).unwrap_or(usize::MAX);
        if before.len() != placed.len() || self.total != total {
            return Ok(true);
        }
        // The same records are shown: the axis changed when one moved, or its group values or
        // search hits did.
        Ok(before.iter().any(|(at, old)| self.rows.get(*at) != Some(old)))
    }

    /// The table's records, shown or not.
    pub fn total(&self) -> usize {
        self.total
    }

    /// The shown records' ids, in order.
    pub fn ids(&self) -> impl Iterator<Item = i64> + '_ {
        self.rows.iter().map(|row| row.id)
    }

    /// The arrangement as an axis: `epoch` names it; the groups whose ids are in `collapsed` are
    /// marked collapsed.
    pub fn axis(&self, epoch: u32, collapsed: &BTreeSet<String>) -> Axis {
        let u32_of = |value: usize| u32::try_from(value).unwrap_or(u32::MAX);
        let mut groups: Vec<GroupHeader> = Vec::new();
        // The headers still open, by depth: indexes into `groups`.
        let mut open: Vec<usize> = Vec::new();
        let mut previous: Option<&[Value]> = None;
        for (at, row) in self.rows.iter().enumerate() {
            let values = row.groups.as_slice();
            let changed = match previous {
                None => (!values.is_empty()).then_some(0),
                Some(previous) => (0..values.len()).find(|depth| !same_value(values.get(*depth), previous.get(*depth))),
            };
            if let Some(depth) = changed {
                for index in open.drain(depth.min(open.len())..) {
                    if let Some(group) = groups.get_mut(index) {
                        group.count = u32_of(at) - group.at;
                    }
                }
                for depth in depth..values.len() {
                    let id = group_id(&values[..=depth]);
                    open.push(groups.len());
                    groups.push(GroupHeader {
                        at: u32_of(at),
                        count: 0,
                        depth: u32_of(depth),
                        collapsed: collapsed.contains(&id),
                        id,
                        value: values[depth].clone(),
                    });
                }
            }
            previous = Some(values);
        }
        for index in open {
            if let Some(group) = groups.get_mut(index) {
                group.count = u32_of(self.rows.len()) - group.at;
            }
        }
        let hits = self.rows.iter().enumerate().flat_map(|(at, row)| row.hits.iter().map(move |field| (u32_of(at), *field))).collect();
        Axis { epoch, total: u32_of(self.total), ids: self.ids().collect(), hits, groups }
    }
}
