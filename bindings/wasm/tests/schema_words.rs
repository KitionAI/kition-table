//! Generation uses the same words and value domains as the engine.
#![allow(clippy::unwrap_used, clippy::indexing_slicing)]
use serde_json::{Value, json};

#[test]
fn schema_words_match_the_tables_domain() {
    let schema: Value = serde_json::from_str(kition_table_kit::MODEL_SCHEMA).unwrap();
    let embedded: Value = serde_json::from_str(include_str!("../../../crates/engine/file/src/kdm/kit-table.schema.json")).unwrap();
    assert_eq!(schema, embedded);
    for ty in ["Base", "Table", "Field", "View"] {
        assert_eq!(schema["types"][ty]["label"], "title");
    }
    assert_eq!(schema["types"]["Dashboard"]["label"], "name");
    for ty in ["Table", "Field"] {
        assert_eq!(schema["types"][ty]["fields"]["name"]["sample"], false);
    }
    assert_eq!(schema["types"]["Field"]["fields"]["type"]["values"], json!(kition_table_file::FIELD_TYPES));
    assert_eq!(schema["types"]["View"]["fields"]["type"]["values"], json!(kition_table_file::VIEW_TYPES));
    assert_eq!(schema["types"]["Record"]["fields"]["createdAt"]["readOnly"], true);
    assert_eq!(schema["types"]["Record"]["fields"]["createdAt"]["display"], json!({"kind":"time","perSecond":1000}));
    assert_eq!(schema["types"]["View"]["fields"]["config"]["members"]["frozen_column_count"]["minimum"], 0);
    kition_kit::kdm::Schema::parse(kition_table_kit::MODEL_SCHEMA).unwrap();
}
