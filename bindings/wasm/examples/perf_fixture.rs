//! Writes the grid's performance fixture (plan section 4.1): one table of `rows` records (default
//! 100,000) in 50 fields of mixed types (text 20, number 8, single select 6, multi select 3, date
//! 4, checkbox 3, rating 2, user 2, attachment 2), with a fixed seed; 5% of the texts hold CJK or
//! Hangul, 1% an emoji, and 10% of the long texts run past one line. Its views: `All rows` (one
//! frozen column), `Grouped` (by `Status 1`, sorted by `Number 1`), and `Tall` (88 px rows).
//!
//! `cargo run --release -p kition-table-kit --example perf_fixture -- <out.ktable> [rows]`
//! (`web/tests/perf.mjs` writes it to `target/perf/` when it is missing; it is never committed).

use kition_kit::KitDocument;
use kition_table_kit::TableDocument;
use serde_json::{Map, Value, json};

/// Records per `transact` request.
const BATCH: usize = 2_000;

/// xorshift64*, from a fixed seed: the same fixture on every machine.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        self.0.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }
    /// A number in `0..n`.
    fn below(&mut self, n: usize) -> usize {
        (self.next() % n.max(1) as u64) as usize
    }
    fn chance(&mut self, percent: usize) -> bool {
        self.below(1000) < percent * 10
    }
    fn pick<'a>(&mut self, items: &[&'a str]) -> &'a str {
        items[self.below(items.len())]
    }
}

const WORDS: [&str; 26] = [
    "alpha", "beta", "gamma", "delta", "review", "shipping", "invoice", "customer", "launch", "budget", "design", "roadmap", "meeting", "draft",
    "final", "update", "report", "sprint", "backlog", "release", "vendor", "contract", "quarter", "region", "pipeline", "Wally",
];
const CJK: [&str; 8] = ["项目进度", "客户反馈", "季度预算", "设计评审", "发布计划", "会议纪要", "東京オフィス", "서울 지사"];
const EMOJI: [&str; 8] = [
    "\u{1F680}",
    "\u{2705}",
    "\u{1F525}",
    "\u{1F4E6}",
    "\u{1F389}",
    "\u{1F44D}\u{1F3FD}",
    "\u{1F1EF}\u{1F1F5}",
    "\u{1F468}\u{200D}\u{1F469}\u{200D}\u{1F467}",
];
const CHOICES: [&str; 8] = ["To do", "In progress", "Blocked", "Review", "Done", "Archived", "待处理", "Hold"];
const TAGS: [&str; 10] = ["red", "green", "blue", "urgent", "backend", "frontend", "design", "ops", "市场", "legal"];
const USERS: [&str; 6] = ["Ada Lovelace", "Grace Hopper", "Alan Turing", "李雷", "Linus Torvalds", "Margaret Hamilton"];

fn words(rng: &mut Rng, n: usize) -> String {
    (0..n).map(|_| rng.pick(&WORDS)).collect::<Vec<_>>().join(" ")
}

/// `least` to `least + more` words, 5% led by CJK or Hangul and 1% ending in an emoji.
fn text(rng: &mut Rng, least: usize, more: usize) -> String {
    let n = least + rng.below(more + 1);
    let words = words(rng, n);
    match rng.below(100) {
        0..5 => format!("{} {words}", rng.pick(&CJK)),
        5 => format!("{words} {}", rng.pick(&EMOJI)),
        _ => words,
    }
}

/// A tiny solid PNG, for the attachment fields.
fn png(rgb: [u8; 3]) -> Result<Vec<u8>, String> {
    let mut bytes = Vec::new();
    {
        let mut encoder = png::Encoder::new(&mut bytes, 32, 32);
        encoder.set_color(png::ColorType::Rgb);
        encoder.set_depth(png::BitDepth::Eight);
        let mut writer = encoder.write_header().map_err(|error| error.to_string())?;
        writer.write_image_data(&rgb.repeat(32 * 32)).map_err(|error| error.to_string())?;
    }
    Ok(bytes)
}

struct Doc(TableDocument);

impl Doc {
    fn call(&mut self, method: &str, params: Value) -> Result<Value, String> {
        let answer = self.0.call(method, &params)?;
        Ok(answer["result"].clone())
    }
}

fn main() -> Result<(), String> {
    let out = std::env::args().nth(1).ok_or("usage: perf_fixture <out.ktable> [rows]")?;
    let rows: usize = std::env::args().nth(2).map_or(Ok(100_000), |n| n.parse().map_err(|_| format!("not a row count: {n}")))?;
    let mut rng = Rng(0x7a6b_0b5e_ed00_0001);
    let mut doc = Doc(TableDocument::create("Perf.ktable", 0, 0)?);
    let table = doc.call("documents.get", json!({}))?["tables"][0].clone();
    let table_id = table["id"].as_i64().ok_or("no table")?;
    let first_view = table["views"][0]["id"].as_i64().ok_or("no view")?;
    doc.call("tables.update", json!({ "tableId": table_id, "title": "Perf" }))?;
    doc.call("views.update", json!({ "tableId": table_id, "viewId": first_view, "title": "All rows" }))?;
    let fields = table["fields"].as_array().cloned().unwrap_or_default();
    let primary =
        fields.iter().find(|field| field["is_primary"] == true).and_then(|field| field["name"].as_str()).ok_or("no primary field")?.to_owned();
    for field in fields.iter().filter(|field| field["is_primary"] != true) {
        doc.call("fields.delete", json!({ "tableId": table_id, "fieldId": field["id"] }))?;
    }

    // (name, type, id)
    let mut made: Vec<(String, &str, i64)> = vec![(primary.clone(), "text", 0)];
    let mut make = |doc: &mut Doc, title: String, kind: &'static str, options: Value| -> Result<(), String> {
        let mut params = json!({ "tableId": table_id, "title": title, "type": kind });
        if !options.is_null() {
            params["options"] = options;
        }
        let created = doc.call("fields.create", params)?;
        let field = if created["field"].is_object() { created["field"].clone() } else { created };
        made.push((field["name"].as_str().ok_or("no field name")?.to_owned(), kind, field["id"].as_i64().unwrap_or(0)));
        Ok(())
    };
    for i in 1..=15 {
        make(&mut doc, format!("Text {i}"), "text", Value::Null)?;
    }
    for i in 1..=4 {
        make(&mut doc, format!("Notes {i}"), "long_text", Value::Null)?;
    }
    for i in 1..=8 {
        make(&mut doc, format!("Number {i}"), "number", Value::Null)?;
    }
    for i in 1..=6 {
        make(&mut doc, format!("Status {i}"), "single_select", json!({ "choices": CHOICES }))?;
    }
    for i in 1..=3 {
        make(&mut doc, format!("Tags {i}"), "multi_select", json!({ "choices": TAGS }))?;
    }
    for i in 1..=4 {
        make(&mut doc, format!("Date {i}"), "date", Value::Null)?;
    }
    for i in 1..=3 {
        make(&mut doc, format!("Done {i}"), "checkbox", Value::Null)?;
    }
    for i in 1..=2 {
        make(&mut doc, format!("Rating {i}"), "rating", json!({ "max": 5, "icon": if i == 1 { "star" } else { "heart" } }))?;
    }
    for i in 1..=2 {
        make(&mut doc, format!("Owner {i}"), "user", Value::Null)?;
    }
    for i in 1..=2 {
        make(&mut doc, format!("Files {i}"), "attachment", Value::Null)?;
    }
    if made.len() != 50 {
        return Err(format!("expected 50 fields, made {}", made.len()));
    }

    let mut blobs = Vec::new();
    for (index, rgb) in [[86, 69, 212], [240, 120, 60], [60, 180, 120], [200, 200, 40]].into_iter().enumerate() {
        let data = base64::Engine::encode(&base64::engine::general_purpose::STANDARD, png(rgb)?);
        let put = doc
            .call("attachments.put", json!({ "tableId": table_id, "name": format!("swatch-{index}.png"), "mimeType": "image/png", "data": data }))?;
        blobs.push(if put["attachment"].is_object() { put["attachment"].clone() } else { put });
    }

    let value = |rng: &mut Rng, kind: &str| -> Option<Value> {
        Some(match kind {
            "text" => json!(text(rng, 1, 4)),
            "long_text" => {
                if rng.chance(10) {
                    json!(format!("{}.\n{}.", text(rng, 25, 25), text(rng, 10, 15)))
                } else {
                    json!(text(rng, 2, 4))
                }
            }
            "number" => {
                if rng.chance(50) {
                    json!(rng.below(100_001))
                } else {
                    json!(rng.below(100_000_000) as f64 / 100.0)
                }
            }
            "single_select" => json!(rng.pick(&CHOICES)),
            "multi_select" => {
                let mut tags: Vec<&str> = (0..1 + rng.below(3)).map(|_| rng.pick(&TAGS)).collect();
                tags.dedup();
                json!(tags)
            }
            "date" => json!(civil_date(19_723 + rng.below(1001) as i64)),
            "checkbox" => json!(rng.chance(40)),
            "rating" => json!(rng.below(6)),
            "user" => {
                let index = rng.below(USERS.len());
                json!([{ "id": format!("u{}", index + 1), "name": USERS[index] }])
            }
            "attachment" => {
                if !rng.chance(60) {
                    return None;
                }
                let n = 1 + rng.below(2);
                Value::Array((0..n).map(|_| blobs[rng.below(blobs.len())].clone()).collect())
            }
            _ => return None,
        })
    };

    let started = kition_table_arrange::Stopwatch::start();
    for start in (0..rows).step_by(BATCH) {
        let records: Vec<Value> = (start..rows.min(start + BATCH))
            .map(|row| {
                let mut values = Map::new();
                for (index, (name, kind, _)) in made.iter().enumerate() {
                    let cell = if index == 0 { Some(json!(format!("Row {:06} {}", row + 1, text(&mut rng, 1, 2)))) } else { value(&mut rng, kind) };
                    if let Some(cell) = cell {
                        values.insert(name.clone(), cell);
                    }
                }
                Value::Object(values)
            })
            .collect();
        doc.call("transact", json!({ "ops": [{ "op": "records.create", "tableId": table_id, "at": null, "records": records }] }))?;
        if (start / BATCH).is_multiple_of(10) {
            eprintln!("{}/{rows} records, {:.1} s", rows.min(start + BATCH), started.elapsed_ms() / 1000.0);
        }
    }

    let status = made.iter().find(|(_, kind, _)| *kind == "single_select").map(|field| field.2).ok_or("no status field")?;
    let number = made.iter().find(|(_, kind, _)| *kind == "number").map(|field| field.2).ok_or("no number field")?;
    doc.call("views.patchConfig", json!({ "tableId": table_id, "viewId": first_view, "frozen_column_count": 1 }))?;
    let grouped = doc.call("views.create", json!({ "tableId": table_id, "title": "Grouped", "type": "grid" }))?;
    let grouped = grouped["id"].as_i64().or_else(|| grouped["view"]["id"].as_i64()).ok_or("no grouped view")?;
    doc.call("views.putGroups", json!({ "tableId": table_id, "viewId": grouped, "items": [{ "field_id": status, "direction": "asc" }] }))?;
    doc.call("views.putSorts", json!({ "tableId": table_id, "viewId": grouped, "items": [{ "field_id": number, "direction": "asc" }] }))?;
    doc.call("views.patchConfig", json!({ "tableId": table_id, "viewId": grouped, "frozen_column_count": 1 }))?;
    let tall = doc.call("views.create", json!({ "tableId": table_id, "title": "Tall", "type": "grid" }))?;
    let tall = tall["id"].as_i64().or_else(|| tall["view"]["id"].as_i64()).ok_or("no tall view")?;
    doc.call("views.patchConfig", json!({ "tableId": table_id, "viewId": tall, "row_height": "extra_tall", "frozen_column_count": 1 }))?;

    let bytes = doc.0.saved_bytes()?;
    let content = doc.0.file().replica().content_snapshot().map_err(|error| error.to_string())?.len();
    eprintln!("the file: {:.1} MB; its content.json: {:.1} MB before DEFLATE", bytes.len() as f64 / 1e6, content as f64 / 1e6);
    if let Some(dir) = std::path::Path::new(&out).parent() {
        std::fs::create_dir_all(dir).map_err(|error| error.to_string())?;
    }
    std::fs::write(&out, &bytes).map_err(|error| error.to_string())?;
    println!(
        "{}",
        json!({ "out": out, "rows": rows, "fields": made.len(), "bytes": bytes.len(), "views": { "all": first_view, "grouped": grouped, "tall": tall }, "seconds": started.elapsed_ms() / 1000.0 })
    );
    Ok(())
}

/// `YYYY-MM-DD` of a day number counted from 1970-01-01 (proleptic Gregorian).
fn civil_date(days: i64) -> String {
    // Howard Hinnant's civil_from_days.
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    format!("{:04}-{m:02}-{d:02}", if m <= 2 { y + 1 } else { y })
}
