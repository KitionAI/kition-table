//! The agent's preview as the headless kit draws it (KHP A39, A24; Kition platform K21): the
//! manifest's `agent.render` command takes `scale`, `maxSize`, `target` (a diagnostic's, or a
//! table, a view, and a range of its rows) and `slide`, and answers a top-level base64 `png`,
//! drawn with the faces the executable embeds: the text a person sees, equal to
//! `conformance/golden/`. Before, the render command was `table.preview.render`, answering
//! `{change, result: {png}}` and taking `focus`, not declared as `agent.render`, so the kits
//! server previewed with the screenshot (no scale, no target); and the executable drew text only
//! when its host set `KITION_FONTS_DIR`, which the kits server does not.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing, clippy::panic)]

use std::path::{Path, PathBuf};
use std::sync::Once;

use base64::Engine as _;
use kition_headless::{Headless, HostFolder, KitInfo};
use kition_kit_server::KitBackend;
use kition_kit_server::OpenRequest;
use kition_kit_server::protocol::Origin;
use kition_table_kit::TableDocument;
use serde_json::{Map, Value, json};

const MANIFEST: &str = include_str!("../../../kit.json");

fn repo() -> &'static Path {
    Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/../.."))
}

fn params(value: Value) -> Map<String, Value> {
    value.as_object().cloned().unwrap_or_default()
}

/// The faces the executable embeds, registered as its `main` does.
fn fonts() {
    static ONCE: Once = Once::new();
    ONCE.call_once(|| {
        assert_eq!(
            kition_table_fonts::register_embedded(),
            kition_table_fonts::CHECKOUT_FILES.len(),
            "the executable embeds the table's faces (build with a kition-fonts checkout)"
        );
    });
}

struct Kit {
    root: PathBuf,
    kit: Headless<TableDocument>,
    id: String,
    command: String,
}

impl Drop for Kit {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

fn open(name: &str) -> Kit {
    fonts();
    let root = std::env::temp_dir().join(format!("kition-table-render-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    let root = std::fs::canonicalize(root).unwrap();
    std::fs::copy(repo().join("conformance/basic.ktable"), root.join("basic.ktable")).unwrap();
    let mut kit = Headless::<TableDocument>::with_replica_port(KitInfo::from_manifest(MANIFEST, &[], &[]).unwrap());
    let folder = HostFolder::new(&root).unwrap();
    kit.set_host(folder.host());
    let id = kit.open(OpenRequest { file: &folder.file("basic.ktable").unwrap(), read_only: false, create: None }).unwrap().id;
    let manifest: Value = serde_json::from_str(MANIFEST).unwrap();
    let command = manifest["agent"]["render"].as_str().expect("kit.json declares agent.render").to_owned();
    Kit { root, kit, id, command }
}

struct Image {
    rgba: Vec<u8>,
    width: u32,
    height: u32,
}

fn decode(bytes: &[u8]) -> Image {
    let mut reader = png::Decoder::new(bytes).read_info().unwrap();
    let mut rgba = vec![0; reader.output_buffer_size()];
    let info = reader.next_frame(&mut rgba).unwrap();
    rgba.truncate(info.buffer_size());
    Image { rgba, width: info.width, height: info.height }
}

impl Kit {
    fn run(&mut self, command: &str, value: Value) -> Result<Value, String> {
        self.kit
            .execute(&self.id, command, params(value), Origin::Agent)
            .map(|executed| executed.result.unwrap_or_default())
            .map_err(|error| error.message)
    }

    /// What the kits server reads: the top-level `png`, decoded.
    fn render(&mut self, value: Value) -> Image {
        let command = self.command.clone();
        let answer = self.run(&command, value.clone()).unwrap_or_else(|error| panic!("{value}: {error}"));
        let png = answer["png"].as_str().unwrap_or_else(|| panic!("no top-level png in {answer}"));
        let image = decode(&base64::engine::general_purpose::STANDARD.decode(png).unwrap());
        assert_eq!((answer["width"].as_u64(), answer["height"].as_u64()), (Some(u64::from(image.width)), Some(u64::from(image.height))));
        image
    }

    /// The refusal's message and its data (the parameters the schema check names).
    fn refused(&mut self, value: Value) -> String {
        let command = self.command.clone();
        match self.kit.execute(&self.id, &command, params(value.clone()), Origin::Agent) {
            Ok(_) => panic!("{value} was not refused"),
            Err(error) => format!("{} {}", error.message, error.data.unwrap_or_default()),
        }
    }
}

fn golden(name: &str) -> Image {
    decode(&std::fs::read(repo().join("conformance/golden").join(name)).unwrap())
}

#[test]
fn the_render_command_draws_the_goldens_with_the_embedded_faces() {
    let mut k = open("goldens");
    assert_eq!(k.command, "table.file.render");
    for (name, value) in [
        ("basic.png", json!({ "scale": 1 })),
        ("basic-scale-2.png", json!({ "scale": 2 })),
        ("basic-target.png", json!({ "target": { "sheet": "Table", "path": "tables/1/records/2/fields/2" }, "scale": 1 })),
    ] {
        let (image, expected) = (k.render(value), golden(name));
        assert_eq!((image.width, image.height), (expected.width, expected.height), "{name}");
        assert!(image.rgba == expected.rgba, "{name}: the picture is not the golden (its text is missing or moved)");
    }
}

#[test]
fn scale_size_and_parts() {
    let mut k = open("parts");
    let whole = k.render(json!({}));
    assert_eq!((whole.width, whole.height), (700, 248));
    let small = k.render(json!({ "scale": 2, "maxSize": 120 }));
    assert_eq!((small.width, small.height), (120, 43));
    assert_eq!(k.render(json!({ "slide": 0 })).rgba, whole.rgba, "a host's part preview: the first table");
    assert_eq!(k.render(json!({ "target": { "path": "tables/1" } })).rgba, whole.rgba);

    // A view: the records it keeps (one of the two is not done).
    let view = k.run("table.views.create", json!({ "tableId": 1, "title": "Open", "type": "grid" })).unwrap()["result"]["id"].clone();
    k.run(
        "table.views.putFilterTree",
        json!({ "tableId": 1, "viewId": view, "tree": { "conjunction": "and", "children": [{ "condition": { "field_id": 2, "operator": "neq", "value": "Done" } }] } }),
    )
    .unwrap();
    let open_work = k.render(json!({ "target": { "path": format!("tables/1/views/{view}") } }));
    assert_eq!((open_work.width, open_work.height), (700, 208), "one row of 40 fewer");

    // A range of rows, counted from 1: rows 3 to 5 of 12.
    for n in 0..10 {
        k.run("table.records.create", json!({ "tableId": 1, "values": { "title": format!("Task {n}") } })).unwrap();
    }
    let all = k.render(json!({}));
    assert_eq!(all.height, 248 + 10 * 40);
    let range = k.render(json!({ "target": { "path": "tables/1", "rows": [3, 5] } }));
    assert_eq!((range.width, range.height), (700, 248 + 40), "three rows");
    assert_ne!(range.rgba, k.render(json!({ "target": { "path": "tables/1", "rows": [1, 3] } })).rgba);

    for (bad, says) in [
        (json!({ "scale": 3 }), "scale"),
        (json!({ "maxSize": 5000 }), "maxSize"),
        (json!({ "target": "tables/1" }), "target"),
        (json!({ "target": { "path": "tables/99" } }), "not found"),
        (json!({ "target": { "path": "tables/1/views/99" } }), "not found"),
        (json!({ "target": { "path": "tables/1", "rows": [0, 2] } }), "rows"),
        (json!({ "target": { "path": "tables/1", "rows": [5, 3] } }), "rows"),
        (json!({ "target": { "path": "tables/1", "rows": [40, 50] } }), "rows"),
        (json!({ "slide": 9 }), "slide"),
    ] {
        let error = k.refused(bad.clone());
        assert!(error.contains(says), "{bad}: {error}");
    }
}
