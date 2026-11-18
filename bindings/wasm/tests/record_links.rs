//! Link values are written with the records: `records.create`, `records.update`
//! and `transact` take a `link_to_record` field's value as a list of row keys, record ids or the
//! `{row_key}` objects `records.list` answers, and link the records. A link field whose
//! `symmetric_field_id` names the target table's field that links back shows the links made from
//! the other side. `records.create` and `records.update` answer `changed`.

#![allow(clippy::unwrap_used)]

use kition_kit::KitDocument;
use kition_table_kit::TableDocument;
use serde_json::{Value, json};

fn call(doc: &mut TableDocument, method: &str, params: Value) -> Value {
    doc.call(method, &params).unwrap_or_else(|error| panic!("{method}: {error}"))["result"].clone()
}

struct Teams {
    doc: TableDocument,
    people: i64,
    teams: i64,
    /// People.teams (to Teams); Teams.members (to People) names it as its symmetric field.
    person_teams: i64,
    /// The row keys of Ada, Bo and Cy.
    keys: Vec<String>,
}

fn teams() -> Teams {
    let mut doc = TableDocument::create("Teams.ktable", 0, 0).unwrap();
    let people = call(
        &mut doc,
        "tables.create",
        json!({ "title": "People", "fields": [{ "name": "name", "title": "Name", "type": "text", "primary": true }] }),
    )["id"]
        .as_i64()
        .unwrap();
    let teams = call(
        &mut doc,
        "tables.create",
        json!({ "title": "Teams", "fields": [{ "name": "team", "title": "Team", "type": "text", "primary": true }] }),
    )["id"]
        .as_i64()
        .unwrap();
    let person_teams = call(
        &mut doc,
        "fields.create",
        json!({ "tableId": people, "name": "teams", "title": "Teams", "type": "link_to_record", "options": { "target_table_id": teams, "allow_multiple": true } }),
    )["id"]
        .as_i64()
        .unwrap();
    call(
        &mut doc,
        "fields.create",
        json!({ "tableId": teams, "name": "members", "title": "Members", "type": "link_to_record",
                "options": { "target_table_id": people, "allow_multiple": true, "symmetric_field_id": person_teams } }),
    );
    let created = call(
        &mut doc,
        "transact",
        json!({ "ops": [{ "op": "records.create", "tableId": people, "records": [{ "name": "Ada" }, { "name": "Bo" }, { "name": "Cy" }] }] }),
    );
    let keys = created["created"].as_array().unwrap().iter().map(|key| key.as_str().unwrap().to_owned()).collect();
    Teams { doc, people, teams, person_teams, keys }
}

/// `{record's primary value: [linked records' display values]}` for a link field of a table.
fn linked(doc: &mut TableDocument, table: i64, primary: &str, field: &str) -> Vec<(String, Vec<String>)> {
    let records = call(doc, "records.list", json!({ "tableId": table }));
    records["items"]
        .as_array()
        .unwrap()
        .iter()
        .map(|record| {
            let values = &record["values"];
            let targets = values[field]
                .as_array()
                .map(|items| items.iter().map(|item| item["display"].as_str().unwrap().to_owned()).collect())
                .unwrap_or_default();
            (values[primary].as_str().unwrap().to_owned(), targets)
        })
        .collect()
}

fn pairs(rows: &[(&str, &[&str])]) -> Vec<(String, Vec<String>)> {
    rows.iter().map(|(name, targets)| ((*name).to_owned(), targets.iter().map(|target| (*target).to_owned()).collect())).collect()
}

#[test]
fn transact_creates_records_with_their_links() {
    let Teams { mut doc, teams, keys, .. } = teams();
    call(
        &mut doc,
        "transact",
        json!({ "ops": [{ "op": "records.create", "tableId": teams, "records": [
            { "team": "Core", "members": [keys[0], keys[1]] },
            { "team": "Docs", "members": [{ "row_key": keys[2], "display": "Cy" }] },
        ] }] }),
    );
    assert_eq!(linked(&mut doc, teams, "team", "members"), pairs(&[("Core", &["Ada", "Bo"]), ("Docs", &["Cy"])]));
}

#[test]
fn records_create_and_update_take_record_ids_and_answer_changed() {
    let Teams { mut doc, people, teams, .. } = teams();
    let ids: Vec<i64> =
        call(&mut doc, "records.list", json!({ "tableId": people }))["items"].as_array().unwrap().iter().map(|r| r["id"].as_i64().unwrap()).collect();
    let core = call(&mut doc, "records.create", json!({ "tableId": teams, "values": { "team": "Core", "members": [ids[0]] } }));
    assert_eq!(core["changed"], json!(true));
    assert_eq!(linked(&mut doc, teams, "team", "members"), pairs(&[("Core", &["Ada"])]));

    let core_id = core["id"].as_i64().unwrap();
    let updated = call(&mut doc, "records.update", json!({ "tableId": teams, "recordId": core_id, "values": { "members": [ids[2], ids[1]] } }));
    assert_eq!(updated["changed"], json!(true));
    assert_eq!(linked(&mut doc, teams, "team", "members"), pairs(&[("Core", &["Cy", "Bo"])]));

    let same = call(&mut doc, "records.update", json!({ "tableId": teams, "recordId": core_id, "values": { "team": "Core" } }));
    assert_eq!(same["changed"], json!(false), "the value is already Core");
    // A key that names no field is refused and changes nothing.
    let unknown = doc.call("records.update", &json!({ "tableId": teams, "recordId": core_id, "values": { "nickname": "C" } }));
    assert!(unknown.is_err_and(|error| error.contains("`values.nickname` names no field")), "a key that names no field is refused");
    assert_eq!(linked(&mut doc, teams, "team", "members"), pairs(&[("Core", &["Cy", "Bo"])]), "a key that names no field changes nothing");

    let cleared = call(&mut doc, "records.update", json!({ "tableId": teams, "recordId": core_id, "values": { "members": null } }));
    assert_eq!(cleared["changed"], json!(true));
    assert_eq!(linked(&mut doc, teams, "team", "members"), pairs(&[("Core", &[])]));

    let missing = doc.call("records.update", &json!({ "tableId": teams, "recordId": core_id, "values": { "members": [999] } }));
    assert!(missing.is_err(), "a record id that names no record is refused: {missing:?}");
}

#[test]
fn a_symmetric_field_shows_links_made_from_the_other_side() {
    let Teams { mut doc, people, teams, person_teams, keys, .. } = teams();
    call(
        &mut doc,
        "transact",
        json!({ "ops": [{ "op": "records.create", "tableId": teams, "records": [{ "team": "Core", "members": [keys[0], keys[1]] }, { "team": "Docs" }] }] }),
    );
    assert_eq!(linked(&mut doc, people, "name", "teams"), pairs(&[("Ada", &["Core"]), ("Bo", &["Core"]), ("Cy", &[])]));

    // From the other side: Cy joins Docs, Ada leaves Core.
    let person_ids: Vec<i64> =
        call(&mut doc, "records.list", json!({ "tableId": people }))["items"].as_array().unwrap().iter().map(|r| r["id"].as_i64().unwrap()).collect();
    let team_keys: Vec<String> = call(&mut doc, "records.list", json!({ "tableId": teams }))["items"]
        .as_array()
        .unwrap()
        .iter()
        .map(|r| r["row_key"].as_str().unwrap().to_owned())
        .collect();
    call(
        &mut doc,
        "links.replace",
        json!({ "tableId": people, "recordId": person_ids[2], "field_id": person_teams, "target_row_keys": [team_keys[1]] }),
    );
    call(&mut doc, "records.update", json!({ "tableId": people, "recordId": person_ids[0], "values": { "teams": [] } }));
    assert_eq!(linked(&mut doc, teams, "team", "members"), pairs(&[("Core", &["Bo"]), ("Docs", &["Cy"])]));
    assert_eq!(linked(&mut doc, people, "name", "teams"), pairs(&[("Ada", &[]), ("Bo", &["Core"]), ("Cy", &["Docs"])]));
}
