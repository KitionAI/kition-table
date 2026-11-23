//! Offline kit-env generation on the pinned kition-sdk, against a reproducible three-row file.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::indexing_slicing)]

use kition_env::{EnvKit, GenerateOptions, generate, stats};
use kition_kits::KitDefinition;
use kition_table_kit::{KitDocument, TableDocument};
use serde_json::{Value, json};
use std::path::PathBuf;

/// The clock, in Unix milliseconds. These tests run natively, never in WebAssembly.
#[allow(clippy::disallowed_methods)]
fn now_millis() -> i64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_millis() as i64
}

#[tokio::test]
async fn waiting_for_sdk_creation_replay_and_required_defaults() {
    // A raw create is given what a normal create gives (a Record's
    // `createdAt` and `createdBy`; the empty `description` of a Table and `formula` of a Field,
    // which receipts leave out), so every creation receipt replays
    // through `table_batch`. The pinned SDK's `kition_env::ops_for` creates with
    // the receipt's id, so the replay makes the same object under the same id, and a saved-state
    // comparison of the two files reports no change at all. The name stays: the case
    // `regressions/issue-c8-table-pin-creation` names it in its steps.
    use kition_kits::{
        KitManager,
        tools::{self, CallOptions, Guard},
    };
    let dir = tempfile::tempdir().unwrap();
    let mut doc = TableDocument::create("Work.ktable", 0, 0).unwrap();
    doc.execute("table.records.create", json!({"tableId":1,"values":{"title":"One"}})).unwrap();
    let bytes = doc.save(None).unwrap();
    let definition = Box::leak(Box::new(KitDefinition::from_manifest_text(include_str!("../../../kit.json")).unwrap()));
    let executable = Some(PathBuf::from(env!("CARGO_BIN_EXE_kition-table")));
    let env = EnvKit::new(definition, executable.clone());
    let start = kition_env::read_snapshot(&env, "Work.ktable", &bytes).await.unwrap();
    let kits = KitManager::new(Some(dir.path().to_owned()), vec![(definition, executable)]);
    let table = doc.file().replica().objects_of_type("Table")[0].to_string();
    let schema = kition_kit::kdm::Schema::parse(include_str!("../../../model/schema.json")).unwrap();
    let mut failures = Vec::new();
    for (ty, place, fields) in [
        ("Table", "root.tables".to_owned(), json!({"name":"review","title":"Review","description":""})),
        ("Field", format!("{table}.fields"), json!({"name":"review","title":"Review","type":"text","required":false,"readonly":false,"formula":""})),
        ("View", format!("{table}.views"), json!({"title":"Review","type":"grid","locked":false})),
        ("Record", format!("{table}.records"), json!({})),
        ("Dashboard", "root.dashboards".to_owned(), json!({"name":"Review"})),
    ] {
        let source = format!("Source{ty}.ktable");
        let target = format!("Replay{ty}.ktable");
        std::fs::write(dir.path().join(&source), &bytes).unwrap();
        let call = async |path: &str, ops: Value| {
            let input = json!({"path":path,"ops":ops,"changes":"all"});
            tools::call_as(&kits, "table_batch", &input, CallOptions { guard: Guard::Off, ..Default::default() }, "env-probe").await
        };
        let earliest = now_millis();
        let answer = match call(&source, json!([{"op":"create","type":ty,"in":place,"fields":fields}])).await.unwrap() {
            Ok(answer) => answer.output,
            Err(error) => {
                failures.push(format!("{ty}: the raw create refused: {}", error.structured()));
                continue;
            }
        };
        let created = &answer["changes"]["created"][0];
        if ty == "Record" {
            let latest = now_millis();
            let at = created["values"]["createdAt"].as_i64();
            if !at.is_some_and(|at| (earliest..=latest).contains(&at)) {
                failures.push(format!("{ty}: createdAt {at:?} is not the time of the create ({earliest}..={latest})"));
            }
        }
        let ops = kition_env::ops_for(&answer["changes"], &start).unwrap();
        std::fs::write(dir.path().join(&target), &bytes).unwrap();
        let replay = match call(&target, json!(ops)).await.unwrap() {
            Ok(replay) => replay.output,
            Err(error) => {
                failures.push(format!("{ty}: the replay of the receipt refused: {}", error.structured()));
                continue;
            }
        };
        let forward = kition_env::read_snapshot(&env, &source, &std::fs::read(dir.path().join(&source)).unwrap()).await.unwrap().canonical();
        let replayed = kition_env::read_snapshot(&env, &target, &std::fs::read(dir.path().join(&target)).unwrap()).await.unwrap().canonical();
        let (made, remade) = (created["id"].as_str().unwrap(), replay["changes"]["created"][0]["id"].as_str().unwrap());
        assert!(forward["objects"][made].as_object().is_some_and(|object| !object.is_empty()), "{ty}: {forward}");
        println!("{ty} created {}", forward["objects"][made]);
        if forward["objects"][made] != replayed["objects"][remade] {
            failures.push(format!("{ty}: replayed {} differs from created {}", replayed["objects"][remade], forward["objects"][made]));
        }
        assert_eq!(made, remade, "{ty}: the replay keeps the receipt's id");
        let changes = kition_kit::kdm::reread::diff_paired(&schema, &forward, &replayed).to_json();
        for class in ["created", "changed", "moved", "deleted", "texts"] {
            assert!(changes[class].as_array().unwrap().is_empty(), "{ty}: {changes}");
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
    kits.shutdown().await;
}

#[tokio::test]
async fn seed_seven_one_hundred_tasks_and_create_classification() {
    let dir = tempfile::tempdir().unwrap();
    let mut doc = TableDocument::create("Work.ktable", 0, 0).unwrap();
    for title in ["One", "Two", "Three"] {
        doc.execute("table.records.create", json!({"tableId":1,"values":{"title":title,"notes":"N"}})).unwrap();
    }
    std::fs::write(dir.path().join("Work.ktable"), doc.save(None).unwrap()).unwrap();
    let definition = Box::leak(Box::new(KitDefinition::from_manifest_text(include_str!("../../../kit.json")).unwrap()));
    let kit = EnvKit::new(definition, Some(PathBuf::from(env!("CARGO_BIN_EXE_kition-table"))));
    let options = GenerateOptions { seed: 7, count: 100, ..Default::default() };
    let generated = generate(&kit, dir.path(), &options).await.unwrap();
    println!("kit-env seed 7 count 100: {}", stats(&generated.tasks));
    println!("kit-env given up: {}; excluded: {:?}", json!(generated.given_up), generated.excluded);
    assert_eq!(generated.tasks.len(), 100, "{:?}", generated.given_up);
    assert!(generated.skipped.is_empty(), "{:?}", generated.skipped);
    // A field's choices are two more lists to create in: enough tasks to reach every type.
    let creates = generate(&kit, dir.path(), &GenerateOptions { count: 40, kinds: Some(vec!["create".into()]), ..options }).await.unwrap();
    // Creation tasks replay with the SDK's raw `direct` contracts and creation replay.
    println!("create-only tasks {}; given up {}", creates.tasks.len(), json!(creates.given_up));
    assert_eq!(creates.tasks.len(), 40, "{:?}", creates.given_up);
    assert!(creates.given_up.is_empty(), "{:?}", creates.given_up);
    assert!(creates.tasks.iter().all(|task| task.kind == "create" && !task.solution.is_empty()));
    let types: std::collections::BTreeSet<_> = creates
        .tasks
        .iter()
        .flat_map(|task| task.ground_truth["created"].as_array().cloned().unwrap_or_default())
        .filter_map(|item| item["type"].as_str().map(str::to_owned))
        .collect();
    assert_eq!(
        types,
        ["Choice", "Dashboard", "Field", "Record", "Table", "View"].map(str::to_owned).into(),
        "every stored type is created and replayed"
    );
}
