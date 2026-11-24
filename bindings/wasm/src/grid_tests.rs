//! The editor's arrangement through the engine (`gridOpen`, `gridSync`, `gridStats`): the axis
//! follows the editor's settings, every kind of change to the document, and the agent's `read`.

use kition_kit::KitDocument;
use kition_table_arrange::{Axis, Hits};
use serde_json::{Value, json};

use crate::TableDocument;

struct Table {
    doc: TableDocument,
    table: i64,
    view: i64,
    title: String,
    score: String,
}

/// A table with records "b" (score 2), "a" (score 3), "c" (score 1), in that order.
fn table() -> Table {
    let mut doc = TableDocument::create("Tasks.ktable", 0, 0).unwrap();
    let document = doc.call("documents.get", &Value::Null).unwrap()["result"].clone();
    let table = document["tables"][0]["id"].as_i64().unwrap();
    let view = document["tables"][0]["views"][0]["id"].as_i64().unwrap();
    let title = document["tables"][0]["fields"].as_array().unwrap().iter().find(|field| field["is_primary"] == json!(true)).unwrap()["name"]
        .as_str()
        .unwrap()
        .to_owned();
    let score = doc.call("fields.create", &json!({ "tableId": table, "title": "Score", "type": "number" })).unwrap()["result"]["name"]
        .as_str()
        .unwrap()
        .to_owned();
    for (name, value) in [("b", 2), ("a", 3), ("c", 1)] {
        doc.call("records.create", &json!({ "tableId": table, "values": { title.clone(): name, score.clone(): value } })).unwrap();
    }
    Table { doc, table, view, title, score }
}

impl Table {
    fn titles(&self, axis: &Axis) -> Vec<String> {
        let records = self.doc.file().list_records(self.table).unwrap();
        axis.ids
            .iter()
            .map(|id| records.iter().find(|record| record.id == *id).unwrap().values[&self.title].as_str().unwrap_or_default().to_owned())
            .collect()
    }

    fn open(&mut self, params: Value) -> Axis {
        let mut params = params;
        params["tableId"] = json!(self.table);
        params["viewId"] = json!(self.view);
        Axis::decode(&self.doc.grid_open(&params).unwrap()).unwrap()
    }

    /// The hits of `count` rows of `axis` from `from`, in the title and the score.
    fn hits(&mut self, axis: &Axis, from: u32, count: u32) -> Hits {
        let fields = self.doc.call("documents.get", &Value::Null).unwrap()["result"]["tables"][0]["fields"].clone();
        let id = |name: &str| fields.as_array().unwrap().iter().find(|field| field["name"] == json!(name)).unwrap()["id"].clone();
        let params = json!({ "epoch": axis.epoch, "from": from, "count": count, "fieldIds": [id(&self.title), id(&self.score)] });
        Hits::decode(&self.doc.grid_hits(&params).unwrap()).unwrap()
    }

    fn sync(&mut self) -> Option<Axis> {
        let bytes = self.doc.grid_sync().unwrap();
        (!bytes.is_empty()).then(|| Axis::decode(&bytes).unwrap())
    }

    /// The titles of the axis a sync answers (it must answer one).
    fn synced(&mut self) -> Vec<String> {
        let axis = self.sync().expect("a new axis");
        self.titles(&axis)
    }

    fn record_id(&self, title: &str) -> i64 {
        self.doc.file().list_records(self.table).unwrap().iter().find(|record| record.values[&self.title] == json!(title)).unwrap().id
    }

    fn sorted_by_title(&self) -> Value {
        json!({ "config": { "sorts": [{ "id": "s", "field_name": self.title, "direction": "asc" }] } })
    }
}

#[test]
fn the_axis_follows_the_editor_s_settings_before_they_are_saved() {
    let mut t = table();
    let plain = t.open(json!({}));
    assert_eq!(t.titles(&plain), ["b", "a", "c"]);
    assert_eq!(plain.total, 3);
    let sorted = t.open(t.sorted_by_title());
    assert_eq!(t.titles(&sorted), ["a", "b", "c"]);
    assert!(sorted.epoch > plain.epoch);
    let filtered = t.open(json!({ "config": { "filter_tree": { "id": "g", "kind": "group", "conjunction": "and", "children": [
        { "id": "c", "kind": "condition", "field_name": t.score, "operator": "isGreater", "value": 1 },
    ] } } }));
    assert_eq!(t.titles(&filtered), ["b", "a"]);
    let searched = t.open(json!({ "query": "A" }));
    assert_eq!(t.titles(&searched), ["a"]);
    // The one cell holding it, asked for apart (`gridHits`): the title of the one record.
    let hits = t.hits(&searched, 0, 10);
    assert_eq!((hits.count, hits.holds(0, 0), hits.holds(0, 1)), (1, true, false));
}

#[test]
fn grid_hits_answer_the_cells_holding_the_search_in_a_run_of_rows_as_they_are_now() {
    let mut t = table();
    // "2" is only b's score.
    let axis = t.open(json!({ "query": "2" }));
    assert_eq!(t.titles(&axis), ["b"]);
    let hits = t.hits(&axis, 0, 10);
    assert_eq!((hits.epoch, hits.from, hits.count, hits.fields), (axis.epoch, 0, 1, 2));
    assert_eq!((hits.holds(0, 0), hits.holds(0, 1)), (false, true));
    // An edit that keeps the record where it is leaves the axis; its hits follow the edit.
    let b = t.record_id("b");
    t.doc.call("records.update", &json!({ "tableId": t.table, "recordId": b, "values": { t.title.clone(): "b2" } })).unwrap();
    assert_eq!(t.sync(), None);
    let hits = t.hits(&axis, 0, 10);
    assert_eq!((hits.holds(0, 0), hits.holds(0, 1)), (true, true));
    // Rows past the last: none.
    assert_eq!(t.hits(&axis, 5, 10).count, 0);
    // Without a search no cell holds it.
    let plain = t.open(json!({}));
    let hits = t.hits(&plain, 0, 10);
    assert_eq!(hits.count, 3);
    assert!((0..3).all(|row| !hits.holds(row, 0) && !hits.holds(row, 1)));
    // An axis that is not the latest is refused.
    let error = t.doc.grid_hits(&json!({ "epoch": axis.epoch, "from": 0, "count": 1, "fieldIds": [] })).unwrap_err();
    assert!(error.contains("stale"), "{error}");
    assert!(t.doc.grid_hits(&json!({ "epoch": plain.epoch, "from": 0, "count": 1 })).is_err());
}

/// The editor's footer sums number fields over the shown records without reading them on the
/// page (`gridSummary`, case table.editor.grid/editor).
#[test]
fn the_summary_counts_and_sums_the_numbers_of_the_shown_records() {
    let mut t = table();
    assert!(t.doc.grid_summary(&json!({ "fields": [t.score.clone()] })).is_err(), "no arrangement yet");
    t.open(json!({}));
    let summary = t.doc.grid_summary(&json!({ "fields": [t.score.clone(), t.title.clone()] })).unwrap();
    assert_eq!(summary, json!([{ "field": t.score, "count": 3, "sum": 6.0 }, { "field": t.title, "count": 0, "sum": 0.0 }]));
    // Only the records the arrangement shows; a cell without a number counts for nothing.
    t.open(json!({ "query": "a" }));
    let summary = t.doc.grid_summary(&json!({ "fields": [t.score.clone()] })).unwrap();
    assert_eq!(summary, json!([{ "field": t.score, "count": 1, "sum": 3.0 }]));
    let a = t.record_id("a");
    t.doc.call("records.update", &json!({ "tableId": t.table, "recordId": a, "values": { t.score.clone(): null } })).unwrap();
    let summary = t.doc.grid_summary(&json!({ "fields": [t.score.clone()] })).unwrap();
    assert_eq!(summary, json!([{ "field": t.score, "count": 0, "sum": 0.0 }]));
    assert!(t.doc.grid_summary(&json!({})).is_err(), "fields are named");
}

#[test]
fn a_sync_moves_an_edited_record_and_answers_nothing_when_the_axis_stays() {
    let mut t = table();
    let params = t.sorted_by_title();
    t.open(params);
    assert_eq!(t.sync(), None);
    // A read changes nothing.
    t.doc.call("records.list", &json!({ "tableId": t.table })).unwrap();
    assert_eq!(t.sync(), None);
    let a = t.record_id("a");
    t.doc.call("records.update", &json!({ "tableId": t.table, "recordId": a, "values": { t.title.clone(): "d" } })).unwrap();
    let moved = t.sync().unwrap();
    assert_eq!(t.titles(&moved), ["b", "c", "d"]);
    // An edit that keeps the order keeps the axis.
    let c = t.record_id("c");
    t.doc.call("records.update", &json!({ "tableId": t.table, "recordId": c, "values": { t.score.clone(): 9 } })).unwrap();
    assert_eq!(t.sync(), None);
    // Undo puts it back.
    t.doc.call("undo", &json!({})).unwrap();
    t.doc.call("undo", &json!({})).unwrap();
    assert_eq!(t.synced(), ["a", "b", "c"]);
    let stats = t.doc.grid_stats();
    assert_eq!(stats["arrange"]["count"], json!(1));
    assert!(stats["relocate"]["count"].as_u64().unwrap() >= 2, "{stats}");
}

#[test]
fn created_and_deleted_records_come_and_go() {
    let mut t = table();
    let params = t.sorted_by_title();
    t.open(params);
    t.doc.call("records.create", &json!({ "tableId": t.table, "values": { t.title.clone(): "aa" } })).unwrap();
    assert_eq!(t.synced(), ["a", "aa", "b", "c"]);
    let b = t.record_id("b");
    t.doc.call("records.delete", &json!({ "tableId": t.table, "recordId": b })).unwrap();
    let axis = t.sync().unwrap();
    assert_eq!(t.titles(&axis), ["a", "aa", "c"]);
    assert_eq!(axis.total, 3);
}

#[test]
fn the_view_s_engine_sorts_and_a_changed_schema_arrange_again() {
    let mut t = table();
    t.open(json!({ "config": {} }));
    let score_id = t.doc.file().get_table(t.table).unwrap().fields.iter().find(|field| field.name == t.score).unwrap().id;
    t.doc.call("views.putSorts", &json!({ "tableId": t.table, "viewId": t.view, "items": [{ "field_id": score_id, "direction": "asc" }] })).unwrap();
    assert_eq!(t.synced(), ["c", "b", "a"]);
    // A field the order reads, deleted: the order falls back to the table's.
    let field = t.doc.file().get_table(t.table).unwrap().fields.iter().find(|field| field.name == t.score).unwrap().id;
    t.doc.call("fields.delete", &json!({ "tableId": t.table, "fieldId": field })).unwrap();
    assert_eq!(t.synced(), ["b", "a", "c"]);
}

#[test]
fn changes_imported_from_a_co_editor_are_arranged() {
    let mut t = table();
    let bytes = t.doc.sealed_save().unwrap();
    let mut peer = TableDocument::open("Tasks.ktable", &bytes).unwrap();
    let params = t.sorted_by_title();
    t.open(params);
    let since = t.doc.version();
    let a = t.record_id("a");
    peer.call("records.update", &json!({ "tableId": t.table, "recordId": a, "values": { t.title.clone(): "z" } })).unwrap();
    let changes = peer.export_changes(&since).unwrap();
    t.doc.import_changes(&changes).unwrap();
    assert_eq!(t.synced(), ["b", "c", "z"]);
}

#[test]
fn collapsed_groups_are_marked_without_arranging_again() {
    let mut t = table();
    let config = json!({ "config": { "groups": [{ "id": "g", "field_name": t.score, "direction": "desc" }] } });
    let open = t.open(config.clone());
    assert_eq!(t.titles(&open), ["a", "b", "c"]);
    assert_eq!(open.groups.iter().map(|group| group.id.as_str()).collect::<Vec<_>>(), ["g0:3", "g0:2", "g0:1"]);
    let mut collapsed = config;
    collapsed["collapsed"] = json!(["g0:2"]);
    let axis = t.open(collapsed);
    assert_eq!(axis.groups.iter().map(|group| group.collapsed).collect::<Vec<_>>(), [false, true, false]);
    assert_eq!(t.doc.grid_stats()["arrange"]["count"], json!(1));
}

#[test]
fn a_sync_before_an_open_and_a_missing_view_are_errors() {
    let mut t = table();
    assert!(t.doc.grid_sync().unwrap_err().contains("gridOpen"));
    assert!(t.doc.grid_open(&json!({ "tableId": t.table, "viewId": 999_999 })).unwrap_err().contains("not found"));
    assert!(t.doc.grid_open(&json!({ "viewId": t.view })).unwrap_err().contains("tableId"));
}

#[test]
fn the_agent_reads_a_view_in_the_editor_s_order() {
    let mut t = table();
    let score_id = t.doc.file().get_table(t.table).unwrap().fields.iter().find(|field| field.name == t.score).unwrap().id;
    t.doc.call("views.putSorts", &json!({ "tableId": t.table, "viewId": t.view, "items": [{ "field_id": score_id, "direction": "desc" }] })).unwrap();
    let axis = t.open(json!({}));
    let read = t.doc.call("read", &json!({ "viewId": t.view })).unwrap()["result"].clone();
    let rows = read["rows"].as_array().or_else(|| read["records"].as_array()).unwrap();
    let ids: Vec<i64> = rows.iter().map(|row| row["id"].as_i64().or_else(|| row["recordId"].as_i64()).unwrap()).collect();
    assert_eq!(ids, axis.ids);
    assert_eq!(t.titles(&axis), ["a", "b", "c"]);
}
