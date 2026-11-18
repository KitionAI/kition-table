//! The grid's layout, checked as structure (which ops, in which faces, where), not as pixels.

use kition_table_display::{Align, DisplayList, Face, HitKind, Layer, Op, Resources};
use kition_table_file::{Field, Record};
use serde_json::{Map, Value, json};

use super::*;
use crate::theme::Theme;

fn fonts() {
    let dir = std::env::var_os("KITION_FONTS_DIR")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../../kition-fonts"));
    let found = kition_table_fonts::register_dir(&dir);
    assert_eq!(found, kition_table_fonts::CHECKOUT_FILES.len(), "the kition-fonts checkout at {} (set KITION_FONTS_DIR)", dir.display());
}

fn field(id: i64, name: &str, kind: &str, options: Value) -> Field {
    Field {
        id,
        name: name.into(),
        title: name.to_uppercase(),
        field_type: kind.into(),
        options: options.as_object().cloned().unwrap_or_default(),
        ..Field::default()
    }
}

fn record(id: i64, values: Value) -> Record {
    Record {
        id,
        document_id: String::new(),
        table_id: 1,
        row_key: String::new(),
        order: id,
        values: values.as_object().cloned().unwrap_or_else(Map::new),
        created_at: 0,
        updated_at: 0,
    }
}

fn column(field: Field) -> Column {
    Column { field, width: DEFAULT_WIDTH }
}

fn view<'a>(columns: &'a [Column], records: &'a [Record], groups: &'a [Field], row_height: f32) -> View<'a> {
    View { columns, records, groups, first: 1, row_height, frozen: 1, reorderable: groups.is_empty(), theme: Theme::LIGHT }
}

/// The ops whose top left lies in `area` (x, y, w, h), described in one line each.
fn ops_in(grid: &Grid, area: Area) -> Vec<String> {
    let inside = |x: f32, y: f32| x >= area.x - 0.01 && x < area.x + area.w && y >= area.y - 0.01 && y < area.y + area.h;
    let list = &grid.list;
    grid.list.ops.iter().filter_map(|op| describe(list, &grid.resources, op).filter(|_| anchor(op).is_some_and(|(x, y)| inside(x, y)))).collect()
}

fn anchor(op: &Op) -> Option<(f32, f32)> {
    Some(match op {
        Op::Rect { x, y, .. }
        | Op::RRect { x, y, .. }
        | Op::RRectStroke { x, y, .. }
        | Op::Text { x, y, .. }
        | Op::Tofu { x, y, .. }
        | Op::Emoji { x, y, .. }
        | Op::Icon { x, y, .. }
        | Op::Image { x, y, .. } => (*x, *y),
        _ => return None,
    })
}

fn hexa(color: [u8; 4]) -> String {
    format!("#{:02X}{:02X}{:02X}", color[0], color[1], color[2])
}

fn describe(list: &DisplayList, resources: &Resources, op: &Op) -> Option<String> {
    Some(match op {
        Op::Rect { w, h, paint, .. } => format!("rect {w}x{h} {}", hexa(list.color(*paint))),
        Op::RRect { w, h, r, paint, .. } => format!("rrect {w}x{h} r{r} {}", hexa(list.color(*paint))),
        Op::RRectStroke { w, h, width, paint, .. } => format!("border {w}x{h} {width} {}", hexa(list.color(*paint))),
        Op::Text { align, face, weight, size, paint, text, .. } => {
            format!("text {face:?} {weight} {size} {align:?} {} '{}'", hexa(list.color(*paint)), list.text(*text))
        }
        Op::Tofu { .. } => "tofu".into(),
        Op::Emoji { .. } => "emoji".into(),
        Op::Icon { icon, size, paint, solid, .. } => {
            format!("icon {} {size}{} {}", resources.icon_name(*icon).unwrap_or("?"), if *solid { " solid" } else { "" }, hexa(list.color(*paint)))
        }
        Op::Image { w, h, .. } => format!("image {w}x{h}"),
        _ => return None,
    })
}

/// One record's row: its cells' ops, per column.
fn one_row(fields: Vec<Field>, values: Value, row_height: f32) -> (Grid, Vec<Vec<String>>) {
    fonts();
    let columns: Vec<Column> = fields.into_iter().map(column).collect();
    let records = vec![record(1, values)];
    let grid = lay_out(&view(&columns, &records, &[], row_height), 0.0, 0.0, None);
    let (_, y, height) = grid.geometry.rows[0].clone();
    let cells = grid.geometry.lefts.iter().map(|left| ops_in(&grid, Area { x: *left, y, w: DEFAULT_WIDTH, h: height })).collect();
    (grid, cells)
}

#[test]
fn geometry_is_the_editors() {
    let columns = vec![column(field(1, "title", "text", json!({}))), Column { field: field(2, "n", "number", json!({})), width: 120.0 }];
    let records: Vec<Record> = (0..3).map(|id| record(id, json!({}))).collect();
    let free = geometry(&view(&columns, &records, &[], 40.0), 100.0);
    assert_eq!(free.row_head, 72.0, "drag handle, checkbox and expand");
    assert_eq!(free.lefts, vec![72.0, 272.0]);
    assert_eq!((free.columns_right, free.width), (392.0, 492.0), "the add-field column is 100 wide");
    assert_eq!(free.frozen_right, 272.0);
    let heights: Vec<(f32, f32)> = free.rows.iter().map(|(_, y, h)| (*y, *h)).collect();
    assert_eq!(heights, vec![(132.0, 40.0), (172.0, 40.0), (212.0, 40.0), (252.0, 32.0)], "header 32, rows, the add row 32");
    assert_eq!(free.height, 32.0 + 3.0 * 40.0 + 32.0);
    assert_eq!(width(&columns, true), 492.0);
    for (name, height) in [(Some("short"), 32.0), (Some("medium"), 40.0), (Some("tall"), 56.0), (Some("extra_tall"), 88.0), (None, 40.0)] {
        assert_eq!(row_height(name), height);
    }
    // Sorted or grouped: no drag handle.
    let groups = vec![columns[0].field.clone()];
    let grouped = geometry(&view(&columns, &records, &groups, 40.0), 0.0);
    assert_eq!(grouped.row_head, 48.0);
}

#[test]
fn groups_have_headers_and_an_add_row_each() {
    let stage = field(2, "stage", "single_select", json!({}));
    let columns = vec![column(field(1, "title", "text", json!({}))), column(stage.clone())];
    let records: Vec<Record> = ["Lead", "Lead", "Won"].iter().enumerate().map(|(id, s)| record(id as i64, json!({ "stage": s }))).collect();
    let groups = vec![stage];
    let rows = rows(&view(&columns, &records, &groups, 40.0));
    assert_eq!(
        rows,
        vec![
            Row::Group { depth: 0, field: 0, record: 0, count: 2 },
            Row::Record { index: 0, number: 1 },
            Row::Record { index: 1, number: 2 },
            Row::Append,
            Row::Group { depth: 0, field: 0, record: 2, count: 1 },
            Row::Record { index: 2, number: 1 },
            Row::Append,
        ],
        "numbers restart in each group"
    );
    fonts();
    let grid = lay_out(&view(&columns, &records, &groups, 40.0), 0.0, 0.0, None);
    let (_, y, h) = grid.geometry.rows[0].clone();
    assert_eq!(h, GROUP_HEADER);
    let header = ops_in(&grid, Area { x: 0.0, y, w: 600.0, h });
    assert!(header.contains(&"rect 448x48 #F8F7F6".to_string()), "{header:?}");
    assert!(header.iter().any(|op| op.starts_with("icon chevron-down 20")), "{header:?}");
    assert!(header.iter().any(|op| op.ends_with("'STAGE'")), "the field's title: {header:?}");
    assert!(header.iter().any(|op| op.starts_with("rrect") && op.contains("x20 r6")), "the value as a chip: {header:?}");
    assert!(header.iter().any(|op| op.ends_with("'Lead'")), "{header:?}");
    assert_eq!(grid.list.hits.iter().filter(|hit| hit.kind == HitKind::GroupToggle).count(), 2);
    assert_eq!(grid.list.hits.iter().filter(|hit| hit.kind == HitKind::AppendRow).count(), 2);
}

#[test]
fn heads_header_and_add_row() {
    fonts();
    let columns = vec![column(Field { is_primary: true, ..field(1, "title", "text", json!({})) }), column(field(2, "due", "date", json!({})))];
    let records = vec![record(7, json!({})), record(8, json!({}))];
    let grid = lay_out(&View { first: 41, ..view(&columns, &records, &[], 40.0) }, 0.0, 0.0, None);
    let header = ops_in(&grid, Area { x: 0.0, y: 0.0, w: 800.0, h: HEADER });
    let expected_header = [
        "rect 472x32 #F8F7F6",
        "border 16x16 1 #797772",
        "icon lock 16 #797772",
        "text Sans 600 11 Center #797772 'Aa'",
        "text Sans 400 13 Left #1A1A1A 'TITLE'",
        "icon chevron-down 12 #797772",
        "icon calendar-days 16 #797772",
        "text Sans 400 13 Left #1A1A1A 'DUE'",
        "icon plus 20 #797772",
    ];
    for op in expected_header {
        assert!(header.contains(&op.to_string()), "{op} in {header:?}");
    }
    // Row heads: numbers from the first record's, in 10 px.
    let heads = ops_in(&grid, Area { x: 0.0, y: HEADER, w: 72.0, h: 200.0 });
    assert!(
        heads.contains(&"text Sans 400 10 Center #797772 '41'".to_string()) && heads.contains(&"text Sans 400 10 Center #797772 '42'".to_string()),
        "{heads:?}"
    );
    assert!(heads.contains(&"icon plus 20 #797772".to_string()), "the add row: {heads:?}");
    // Lines: one horizontal and one vertical batch, and the frozen divider.
    let lines: Vec<&Op> = grid.list.ops.iter().filter(|op| matches!(op, Op::Lines { .. })).collect();
    assert_eq!(lines.len(), 3);
    assert_eq!(grid.list.header.key.layer, Layer::View);
}

#[test]
fn text_cells_center_wrap_and_cut() {
    let title = field(1, "title", "text", json!({}));
    let (_, cells) = one_row(vec![title.clone()], json!({ "title": "Plan -> ship != done" }), 40.0);
    assert_eq!(cells[0], vec!["text Sans 400 13 Center #1A1A1A 'Plan -> ship != done'"], "one run, centred, ligature candidates kept");
    let long = "A very long title that does not fit into its column and is cut with an ellipsis";
    let (grid, cells) = one_row(vec![title.clone()], json!({ "title": long }), 40.0);
    assert_eq!(cells[0].len(), 1);
    assert!(cells[0][0].ends_with("\u{2026}'"), "{:?}", cells[0]);
    let width = grid.list.ops.iter().find_map(|op| if let Op::Text { width, size: 13.0, .. } = op { Some(*width) } else { None }).unwrap_or(0.0);
    assert!(width <= DEFAULT_WIDTH - 16.0 - text::SLACK, "cut 1 px short of the cell's room: {width}");
    // Tall rows wrap: 56 px shows two lines, 88 px three.
    for (height, lines) in [(56.0, 2), (88.0, 3), (32.0, 1)] {
        let (_, cells) = one_row(vec![title.clone()], json!({ "title": long }), height);
        assert_eq!(cells[0].len(), lines, "{height}: {:?}", cells[0]);
        assert!(cells[0].iter().all(|op| op.starts_with("text Sans 400 13 Center")));
    }
    // A URL in the info color, a link in the brand.
    let (_, cells) = one_row(
        vec![field(1, "site", "url", json!({})), field(2, "rel", "link_to_record", json!({}))],
        json!({ "site": "https://kition.ai", "rel": "Deal 1" }),
        40.0,
    );
    assert_eq!(cells[0], vec!["text Sans 400 13 Center #2178C0 'https://kition.ai'"]);
    assert_eq!(cells[1], vec!["text Sans 400 13 Center #0C8375 'Deal 1'"]);
    // Numbers and dates as the editor writes them.
    let (_, cells) =
        one_row(vec![field(1, "n", "number", json!({})), field(2, "d", "date", json!({}))], json!({ "n": 1250.5, "d": "2026-12-01" }), 40.0);
    assert_eq!(
        (cells[0][0].as_str(), cells[1][0].as_str()),
        ("text Sans 400 13 Center #1A1A1A '1250.5'", "text Sans 400 13 Center #1A1A1A '2026/12/01'")
    );
}

#[test]
fn mixed_scripts_split_into_runs_of_one_face() {
    let title = field(1, "title", "text", json!({}));
    let (_, cells) = one_row(vec![title.clone()], json!({ "title": "季度计划 Q4" }), 40.0);
    assert_eq!(cells[0], vec!["text Fallback 400 13 Left #1A1A1A '季度计划 '", "text Sans 400 13 Left #1A1A1A 'Q4'"]);
    let (_, cells) = one_row(vec![title.clone()], json!({ "title": "한국어" }), 40.0);
    assert_eq!(cells[0], vec!["text Hangul 400 13 Center #1A1A1A '한국어'"]);
    let (_, cells) = one_row(vec![title.clone()], json!({ "title": "Go 🚀" }), 40.0);
    assert_eq!(cells[0], vec!["text Sans 400 13 Left #1A1A1A 'Go '", "emoji"]);
    // Sinhala is in no face: boxes, never a run the page would set in a system font.
    let (_, cells) = one_row(vec![title], json!({ "title": "a \u{0D9A}\u{0D9A}" }), 40.0);
    assert_eq!(cells[0], vec!["text Sans 400 13 Left #1A1A1A 'a '", "tofu", "tofu"]);
}

#[test]
fn typed_cells() {
    let fields = vec![
        field(1, "done", "checkbox", json!({})),
        field(2, "open", "checkbox", json!({})),
        field(3, "stage", "single_select", json!({})),
        field(4, "tags", "multi_select", json!({})),
        field(5, "stars", "rating", json!({ "max": 5 })),
        field(6, "owner", "user", json!({})),
        field(7, "files", "attachment", json!({})),
        field(8, "go", "button", json!({ "label": "Run" })),
        field(9, "love", "rating", json!({ "max": 3, "icon": "heart" })),
    ];
    let values = json!({
        "done": true, "open": false, "stage": "Lead", "tags": ["urgent", "ops"], "stars": 3,
        "owner": [{ "id": "u1", "name": "ada" }],
        "files": [{ "id": "a1", "name": "photo.png", "mime_type": "image/png" }, { "id": "a2", "name": "brief.pdf", "mime_type": "application/pdf" }],
        "go": null, "love": 1,
    });
    let (grid, cells) = one_row(fields, values, 40.0);
    assert_eq!(cells[0], vec!["rrect 20x20 r4 #0C8375", "icon check 14 #FFFFFF"], "checked");
    assert_eq!(cells[1], vec!["border 20x20 1 #797772"], "unchecked");
    assert_eq!(cells[2].len(), 2, "one chip: {:?}", cells[2]);
    assert!(cells[2][0].starts_with("rrect") && cells[2][0].contains("x20 r6") && cells[2][1].ends_with("Left #92400E 'Lead'"), "{:?}", cells[2]);
    assert_eq!(cells[3].iter().filter(|op| op.starts_with("rrect")).count(), 2, "two chips: {:?}", cells[3]);
    let stars: Vec<&String> = cells[4].iter().collect();
    assert_eq!(stars.len(), 5);
    assert_eq!(stars.iter().filter(|op| op.as_str() == "icon star 16 solid #F59E0B").count(), 3, "{stars:?}");
    assert_eq!(stars.iter().filter(|op| op.as_str() == "icon star 16 solid #E6E3E0").count(), 2);
    assert_eq!(cells[5].len(), 4, "a pill, an avatar, the initial, the name: {:?}", cells[5]);
    assert!(
        cells[5][0].starts_with("rrect")
            && cells[5][0].contains("x24 r12")
            && cells[5][2].ends_with("Center #1A1A1A 'A'")
            && cells[5][3].ends_with("'ada'")
    );
    assert_eq!(cells[6][0], "image 34x34", "a picture: {:?}", cells[6]);
    assert!(cells[6][1].starts_with("rrect ") && cells[6][1].ends_with("x20 r6 #F6F5F4"), "a file's chip: {:?}", cells[6]);
    assert_eq!(&cells[6][2..], ["icon paperclip 14 #797772", "text Sans 400 12 Left #1A1A1A 'brief.pdf'"], "a file");
    assert!(
        cells[7][0].starts_with("rrect") && cells[7][0].contains("x24 r4 #EEF6F5") && cells[7][1] == "text Sans 500 12 Center #0C8375 'Run'",
        "{:?}",
        cells[7]
    );
    assert_eq!(cells[8].iter().filter(|op| op.starts_with("icon heart 16 solid")).count(), 3);
    // Hit parts for what a click acts on.
    let kinds = |kind: HitKind| grid.list.hits.iter().filter(|hit| hit.kind == kind).count();
    assert_eq!((kinds(HitKind::Checkbox), kinds(HitKind::Rating), kinds(HitKind::Attachment), kinds(HitKind::Button)), (2, 8, 2, 1));
    assert_eq!(kinds(HitKind::ColumnMenu), 9);
}

#[test]
fn focus_outlines_a_cell_or_a_column_header() {
    fonts();
    let columns = vec![column(field(1, "a", "text", json!({}))), column(field(2, "b", "text", json!({})))];
    let records = vec![record(5, json!({})), record(6, json!({}))];
    let cell = lay_out(&view(&columns, &records, &[], 40.0), 10.0, 0.0, Some(Focus { field: Some(2), record: Some(6) }));
    assert_eq!(cell.focus, Some(Area { x: 272.0, y: 10.0 + 32.0 + 40.0, w: 200.0, h: 40.0 }));
    assert!(matches!(cell.list.ops.last(), Some(Op::RRectStroke { width: 2.0, .. })));
    let header = lay_out(&view(&columns, &records, &[], 40.0), 0.0, 0.0, Some(Focus { field: Some(1), record: None }));
    assert_eq!(header.focus, Some(Area { x: 72.0, y: 0.0, w: 200.0, h: 32.0 }));
    let none = lay_out(&view(&columns, &records, &[], 40.0), 0.0, 0.0, Some(Focus { field: Some(9), record: None }));
    assert_eq!(none.focus, None);
}

#[test]
fn the_list_survives_its_bytes_and_draws() {
    fonts();
    let columns = vec![column(field(1, "title", "text", json!({}))), column(field(2, "stars", "rating", json!({})))];
    let records = vec![record(1, json!({ "title": "季度 plan 🚀", "stars": 2 }))];
    let grid = lay_out(&view(&columns, &records, &[], 40.0), 0.0, 900.0, None);
    assert_eq!(grid.width, 900.0);
    let decoded = kition_table_display::decode(&kition_table_display::encode(&grid.list)).expect("decodes");
    assert_eq!(decoded, grid.list);
    let pixels = kition_table_display::raster(&decoded, &grid.resources, 0.0, 0.0, 1.0, 900, grid.geometry.height as u32);
    let open: Vec<usize> = pixels.as_chunks::<4>().0.iter().enumerate().filter(|(_, px)| px[3] != 255).map(|(at, _)| at).take(5).collect();
    assert!(open.is_empty(), "the grid covers its area: {open:?} of {}x{}", 900, grid.geometry.height);
    assert!(grid.list.ops.iter().any(|op| matches!(op, Op::Text { align: Align::Left, face: Face::Fallback, .. })));
}

/// regressions/issue-file-chip: a file that is not a picture is a chip with a paperclip and its
/// name (cut to the cell), as the picture drew it before the grid moved to the display list.
#[test]
fn a_file_is_a_chip_with_its_name() {
    let files = field(7, "files", "attachment", json!({}));
    let brief = json!({ "url": "kition-blob:00ff", "name": "brief.pdf", "mimeType": "application/pdf" });
    let (grid, cells) = one_row(vec![files.clone()], json!({ "files": [brief] }), 40.0);
    assert_eq!(cells[0].len(), 3, "a chip, a paperclip, the name: {:?}", cells[0]);
    assert!(cells[0][0].starts_with("rrect ") && cells[0][0].ends_with("x20 r6 #F6F5F4"), "{:?}", cells[0]);
    assert_eq!(cells[0][1], "icon paperclip 14 #797772");
    assert_eq!(cells[0][2], "text Sans 400 12 Left #1A1A1A 'brief.pdf'");
    let hits: Vec<_> = grid.list.hits.iter().filter(|hit| hit.kind == HitKind::Attachment).collect();
    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].h, 20.0, "the chip is what a click opens");

    // A long name is cut to the cell, with an ellipsis.
    let long = json!({ "url": "kition-blob:01ff", "name": "the quarterly brief for the board meeting.pdf", "mimeType": "application/pdf" });
    let (grid, cells) = one_row(vec![files], json!({ "files": [long] }), 40.0);
    assert!(cells[0][2].ends_with("…'"), "{:?}", cells[0]);
    let chip = grid.list.ops.iter().find_map(|op| match op {
        Op::RRect { x, w, h: 20.0, .. } => Some(x + w),
        _ => None,
    });
    let right = grid.geometry.lefts[0] + DEFAULT_WIDTH - 8.0;
    assert!(chip.is_some_and(|end| end <= right + 0.01), "the chip stays in the cell: {chip:?} > {right}");
}
