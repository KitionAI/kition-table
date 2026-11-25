//! `kit.json` `extras` lists every method the engine's `TableDocument` class exports beyond the
//! engine contract, with its parameters (Kition platform K6), so a host reaches each through
//! `invoke` without guessing. The methods are read from the binding's source: every `pub fn` of an
//! `impl` block that `#[wasm_bindgen(js_class = "TableDocument", ...)]` exports, by its `js_name`
//! (or its Rust name). Getters (`hasReplica`, `docId`) are properties, not methods, and the
//! contract's members (contract 1's, which `export_kit_document!` adds, and contract 2's session
//! members, which `src/web.rs` adds by hand) are left out. Each method must be in `extras` with as
//! many parameters as it takes, and each extra must be such a method (kition-sdk's
//! `tools/check-manifest.mjs --module` checks the second direction on a built module too).

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing, clippy::panic)]

use std::collections::BTreeMap;
use std::path::Path;

/// Engine contract 2's session members the binding writes by hand (Kition platform
/// `docs/platform/kits/engine.md`: the session members, and `blobs`, `addBlobs` and `blobsSince`
/// of kits whose documents carry media). Contract 1's members come from `export_kit_document!` and
/// are not in the binding's source.
const CONTRACT: &[&str] = &[
    "version",
    "exportChanges",
    "importChanges",
    "snapshot",
    "undo",
    "redo",
    "canUndo",
    "canRedo",
    "executeAs",
    "openWithReplica",
    "blobs",
    "addBlobs",
    "blobsSince",
    // kition-sdk's `export_kit_session!` members (engine contract 2), which the table adds by hand.
    "modelDescribe",
    "modelBatch",
    "mergeChanges",
    "compareChanges",
    "restoreState",
    "changesSince",
    "modelReply",
];

/// The `TableDocument` methods the binding's source exports, by their JavaScript names, with the
/// number of parameters each takes (the receiver not counted), and the file each is in.
fn exported() -> BTreeMap<String, (usize, String)> {
    let dir = Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/src"));
    let mut methods = BTreeMap::new();
    let mut files: Vec<_> = std::fs::read_dir(dir).unwrap().map(|entry| entry.unwrap().path()).collect();
    files.sort();
    for path in files.iter().filter(|path| path.extension().is_some_and(|extension| extension == "rs")) {
        let source = std::fs::read_to_string(path).unwrap();
        let name = path.file_name().unwrap().to_string_lossy().into_owned();
        let lines: Vec<&str> = source.lines().collect();
        let mut at = 0;
        while at < lines.len() {
            let line = lines[at];
            at += 1;
            if !(line.trim_start().starts_with("#[wasm_bindgen(") && line.contains("js_class = \"TableDocument\"")) {
                continue;
            }
            // The `impl` block under the attribute, to its closing brace at the same indentation.
            let head = lines[at];
            assert!(head.trim_start().starts_with("impl "), "{name}: the js_class attribute is not on an impl block: {head:?}");
            let end = format!("{}}}", &head[..head.len() - head.trim_start().len()]);
            at += 1;
            let mut attribute = String::new();
            while at < lines.len() && lines[at] != end {
                let trimmed = lines[at].trim_start();
                if trimmed.starts_with("#[wasm_bindgen(") {
                    attribute = trimmed.to_owned();
                } else if let Some(rest) = trimmed.strip_prefix("pub fn ") {
                    // The signature to its closing parenthesis (rustfmt may wrap a long one).
                    let mut signature = rest.to_owned();
                    while !balanced(&signature) {
                        at += 1;
                        signature.push_str(lines[at].trim());
                    }
                    let rust = &signature[..signature.find(['(', '<']).unwrap()];
                    let js = js_name(&attribute).unwrap_or_else(|| rust.to_owned());
                    let getter = attribute.contains("getter");
                    attribute.clear();
                    if !getter {
                        let previous = methods.insert(js.clone(), (parameters(&signature), name.clone()));
                        assert!(previous.is_none(), "{js} is exported twice");
                    }
                } else if !trimmed.starts_with("///") && !trimmed.starts_with("#[") {
                    attribute.clear();
                }
                at += 1;
            }
            assert!(at < lines.len(), "{name}: the exported impl block does not end");
        }
    }
    methods
}

/// Whether the signature's parameter list is closed.
fn balanced(signature: &str) -> bool {
    signature.find('(').is_some_and(|open| {
        let mut depth = 0i32;
        for c in signature[open..].chars() {
            depth += match c {
                '(' => 1,
                ')' => -1,
                _ => 0,
            };
            if depth == 0 {
                return true;
            }
        }
        false
    })
}

/// The `js_name` of a `#[wasm_bindgen(...)]` attribute.
fn js_name(attribute: &str) -> Option<String> {
    let rest = &attribute[attribute.find("js_name")? + "js_name".len()..];
    let rest = rest.trim_start().strip_prefix('=')?.trim_start();
    Some(rest.chars().take_while(|c| c.is_alphanumeric() || *c == '_').collect())
}

/// The parameters a signature takes, `self` not counted.
fn parameters(signature: &str) -> usize {
    let open = signature.find('(').unwrap();
    let mut depth = 0i32;
    let mut current = String::new();
    let mut list = Vec::new();
    for c in signature[open + 1..].chars() {
        match c {
            '(' | '<' | '[' => depth += 1,
            ')' | '>' | ']' if depth > 0 => depth -= 1,
            ')' => break,
            ',' if depth == 0 => {
                list.push(std::mem::take(&mut current));
                continue;
            }
            _ => {}
        }
        current.push(c);
    }
    list.push(current);
    list.iter().map(|parameter| parameter.trim()).filter(|parameter| !parameter.is_empty() && !parameter.ends_with("self")).count()
}

#[test]
fn kit_json_extras_list_every_method_beyond_the_contract() {
    let manifest: serde_json::Value = serde_json::from_str(include_str!("../../../kit.json")).unwrap();
    let extras = manifest["extras"].as_object().expect("kit.json has extras");
    let methods = exported();
    // The scan reads the binding (a scan that found nothing would pass anything).
    for known in ["call", "gridBands", "gridMeasure", "gridOpticalSizing", "executeAs"] {
        assert!(methods.contains_key(known), "the scan of bindings/wasm/src found no {known}: {:?}", methods.keys().collect::<Vec<_>>());
    }
    let mut problems = Vec::new();
    for (method, (count, file)) in &methods {
        if CONTRACT.contains(&method.as_str()) {
            continue;
        }
        match extras.get(method) {
            None => problems.push(format!("{method} (src/{file}) is exported but not in kit.json extras")),
            Some(extra) => {
                let declared = extra["params"].as_array().map_or(0, Vec::len);
                if declared != *count {
                    problems.push(format!("{method} (src/{file}) takes {count} parameters, kit.json extras declares {declared}"));
                }
            }
        }
    }
    for extra in extras.keys() {
        if !methods.contains_key(extra) || CONTRACT.contains(&extra.as_str()) {
            problems.push(format!("kit.json extras.{extra} is not a method the binding exports beyond the contract"));
        }
    }
    assert!(problems.is_empty(), "kit.json extras and the binding disagree (Kition platform K6):\n{}", problems.join("\n"));
}

#[test]
fn the_scan_reads_names_and_parameters_as_the_binding_writes_them() {
    assert_eq!(js_name("#[wasm_bindgen(js_name = gridOpticalSizing)]").as_deref(), Some("gridOpticalSizing"));
    assert_eq!(js_name("#[wasm_bindgen(getter, js_name = docId)]").as_deref(), Some("docId"));
    assert_eq!(js_name("#[wasm_bindgen(js_class = \"TableDocument\")]"), None);
    assert_eq!(parameters("grid_raster(&mut self, epoch: u32, layer: u32, col_band: u32, row_band: u32, dpr: f64) -> Result<Vec<u8>, JsError> {"), 5);
    assert_eq!(parameters("grid_clear(&mut self, params: &str, ids: &[f64]) -> Result<String, JsError> {"), 2);
    assert_eq!(parameters("grid_sync(&mut self) -> Result<Vec<u8>, JsError> {"), 0);
    assert_eq!(parameters("open_with_replica(name: &str, bytes: &[u8], snapshot: &[u8], blobs: &[u8]) -> Result<WebDocument, JsError> {"), 4);
    assert!(!balanced("grid_bands(&mut self, epoch: u32,"));
    assert!(balanced("grid_bands(&mut self, epoch: u32, keys: &[u32]) -> Result<Vec<u8>, JsError> {"));
}
