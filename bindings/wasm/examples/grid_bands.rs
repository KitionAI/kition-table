//! Times the grid's bands (`gridBands`) of a large table natively: the body bands the scrolling
//! test (`web/tests/perf.mjs`) times, never made before (cold) and again (warm), and the bands of
//! a whole viewport after a jump (the row heads, the frozen columns and the first strip of two row
//! bands), exact and as the drafts the page asks for while the scroll bar is dragged.
//!
//! `cargo run --release -p kition-table-kit --example grid_bands -- <table.ktable>`, with the
//! grid's performance fixture (`examples/perf_fixture.rs`, 100,000 records in 50 fields).

use kition_kit::KitDocument;
use kition_table_arrange::Stopwatch;
use kition_table_kit::TableDocument;
use serde_json::json;

fn p95(values: &mut [f64]) -> f64 {
    values.sort_by(f64::total_cmp);
    values.get((values.len().saturating_sub(1) as f64 * 0.95).round() as usize).copied().unwrap_or_default()
}

fn main() -> Result<(), String> {
    let path = std::env::args().nth(1).ok_or("usage: grid_bands <table.ktable>")?;
    let bytes = std::fs::read(&path).map_err(|error| format!("{path}: {error}"))?;
    let mut doc = TableDocument::open("Perf.ktable", &bytes)?;
    let table = doc.call("file.get", &json!({}))?["result"]["tables"][0].clone();
    let table_id = table["id"].as_i64().ok_or("no table")?;
    let view = table["views"][0]["id"].as_i64().ok_or("no view")?;
    let axis = doc.grid_open(&json!({ "tableId": table_id, "viewId": view }))?;
    let axes = doc.grid_axes(&axis, &json!({ "tableId": table_id, "viewId": view }))?;
    let epoch = axes["epoch"].as_u64().ok_or("no epoch")? as u32;
    let row_bands = doc.grid_stats()["bands"]["rowBands"].as_u64().unwrap_or_default() as u32;
    let mut time = |keys: &[u32], draft: bool| -> Result<f64, String> {
        let watch = Stopwatch::start();
        doc.grid_bands(epoch, keys, 2.0, 0, draft)?;
        Ok(watch.elapsed_ms())
    };
    let keys: Vec<[u32; 3]> = (0..24).map(|i| [0, 0, row_bands / 4 + i * (row_bands / 40)]).collect();
    for label in ["cold", "warm"] {
        let mut made = Vec::new();
        for key in &keys {
            made.push(time(key, false)?);
        }
        println!("body band {label}: p95 {:.2} ms, mean {:.2} ms", p95(&mut made), made.iter().sum::<f64>() / made.len() as f64);
    }
    for (label, draft, offset) in [("exact", false, 1), ("drafts", true, 2)] {
        let mut jumps = Vec::new();
        for i in 0..24 {
            let band = row_bands / 3 + i * (row_bands / 50) + offset * 3;
            jumps.push(time(&[4, 0, band, 1, 0, band, 0, 0, band, 4, 0, band + 1, 1, 0, band + 1, 0, 0, band + 1], draft)?);
        }
        println!(
            "viewport after a jump (6 bands, {label}): p95 {:.2} ms, mean {:.2} ms",
            p95(&mut jumps),
            jumps.iter().sum::<f64>() / jumps.len() as f64
        );
    }
    Ok(())
}
