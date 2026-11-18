use super::*;

/// A list with every op, a palette, strings (with CJK), and hits.
pub(crate) fn every_op() -> DisplayList {
    let mut list = DisplayList::new(Header {
        flags: 0,
        epoch: 7,
        theme: 1,
        resources: 3,
        key: Key { layer: Layer::Frozen, col_band: 2, row_band: 70_000 },
        origin: (0.0, 1280.0),
        size: (2048.0, 1280.0),
    });
    let ink = list.paint([26, 26, 26, 255]);
    let line = list.paint([230, 227, 224, 255]);
    let soft = list.paint([12, 131, 117, 40]);
    assert_eq!(list.paint([26, 26, 26, 255]), ink, "a color is one palette entry");
    let hello = list.string("Plan -> ship");
    let cjk = list.string("季度计划");
    list.push(Op::Rect { x: 0.0, y: 1280.0, w: 200.0, h: 40.0, paint: soft, snap: true });
    list.push(Op::RRect { x: 8.5, y: 1290.0, w: 40.0, h: 20.0, r: 6.0, paint: soft });
    list.push(Op::RRectStroke { x: 1.0, y: 1281.0, w: 18.0, h: 18.0, r: 4.0, width: 1.5, paint: ink });
    list.push(Op::Lines {
        axis: Axis::Vertical,
        paint: line,
        lines: vec![Line { pos: 199.0, from: 1280.0, to: 1320.0 }, Line { pos: 399.0, from: 1280.0, to: 1320.0 }],
    });
    list.push(Op::Lines { axis: Axis::Horizontal, paint: line, lines: Vec::new() });
    list.push(Op::Text {
        x: 100.0,
        y: 1304.5,
        align: Align::Center,
        face: Face::Sans,
        weight: 500,
        size: 13.0,
        paint: ink,
        text: hello,
        width: 74.25,
    });
    list.push(Op::Text { x: 8.0, y: 1304.5, align: Align::Right, face: Face::Fallback, weight: 400, size: 13.0, paint: ink, text: cjk, width: 52.0 });
    list.push(Op::Tofu { x: 60.0, y: 1294.0, w: 7.8, h: 10.0, paint: ink });
    list.push(Op::Emoji { atlas: 12, x: 70.0, y: 1304.5, size: 13.0 });
    list.push(Op::Icon { icon: 4, x: 8.0, y: 1288.0, size: 16.0, stroke: 1.5, paint: ink, solid: true });
    list.push(Op::Image { key: 0xDEAD_BEEF, x: 210.0, y: 1284.0, w: 32.0, h: 32.0, fit: Fit::Contain, radius: 4.0 });
    list.push(Op::Shadow { x: 199.0, y: 1280.0, w: 4.0, h: 40.0, r: 0.0, blur: 3.0, paint: line });
    list.push(Op::Clip { x: 0.0, y: 1280.0, w: 2048.0, h: 1280.0 });
    list.push(Op::Unclip);
    list.hits.push(Hit { kind: HitKind::Rating, part: 0, index: 3, col: 5, row: 70_001, x: 1.0, y: 2.0, w: 15.0, h: 15.0 });
    list.hits.push(Hit { kind: HitKind::AppendColumn, part: 1, index: 0, col: u16::MAX, row: u32::MAX, x: 0.0, y: 0.0, w: 100.0, h: 32.0 });
    list
}

#[test]
fn a_list_round_trips_through_its_bytes() {
    let list = every_op();
    let bytes = encode(&list);
    assert_eq!(bytes.len(), list.bytes());
    assert_eq!(bytes.len() % 4, 0, "4-byte aligned");
    assert_eq!(&bytes[..4], b"KTDL");
    assert_eq!(decode(&bytes), Ok(list.clone()));
    assert_eq!(
        list.text(match &list.ops[6] {
            Op::Text { text, .. } => *text,
            _ => unreachable!(),
        }),
        "季度计划"
    );
    // An empty list too.
    let empty = DisplayList::default();
    assert_eq!(encode(&empty).len(), HEADER_BYTES);
    assert_eq!(decode(&encode(&empty)), Ok(empty));
}

#[test]
fn the_bytes_are_the_documented_layout() {
    let list = every_op();
    let bytes = encode(&list);
    let u16_at = |at: usize| u16::from_le_bytes([bytes[at], bytes[at + 1]]);
    let u32_at = |at: usize| u32::from_le_bytes([bytes[at], bytes[at + 1], bytes[at + 2], bytes[at + 3]]);
    let f32_at = |at: usize| f32::from_le_bytes([bytes[at], bytes[at + 1], bytes[at + 2], bytes[at + 3]]);
    // Header.
    assert_eq!((u16_at(4), u16_at(6), u32_at(8), u16_at(12), u16_at(14)), (1, 0, 7, 1, 3));
    assert_eq!((bytes[16], bytes[17], u32_at(20)), (Layer::Frozen as u8, 2, 70_000));
    assert_eq!((f32_at(24), f32_at(28), f32_at(32), f32_at(36)), (0.0, 1280.0, 2048.0, 1280.0));
    assert_eq!((u16_at(40), u32_at(44), u32_at(48), u32_at(52)), (3, 14, list.strings.len() as u32, 2));
    // Palette: RGBA bytes in order.
    assert_eq!(&bytes[56..60], &[26, 26, 26, 255]);
    // Strings, padded to 4.
    let strings = 56 + 12;
    assert_eq!(&bytes[strings..strings + 12], "Plan -> ship".as_bytes());
    let ops = strings + list.strings.len().next_multiple_of(4);
    // The first op: a snapped rect.
    assert_eq!((bytes[ops], bytes[ops + 1], u16_at(ops + 2), f32_at(ops + 12)), (0x01, 1, 2, 200.0));
    // Hits are the last 2 x 28 bytes.
    let hits = bytes.len() - 2 * HIT_BYTES;
    assert_eq!((bytes[hits], u16_at(hits + 2), u16_at(hits + 4), u32_at(hits + 8)), (HitKind::Rating as u8, 3, 5, 70_001));
}

#[test]
fn other_versions_and_broken_bytes_are_refused() {
    let bytes = encode(&every_op());
    let mut other = bytes.clone();
    other[4] = 2;
    assert_eq!(decode(&other), Err(DecodeError::Version(2)));
    assert_eq!(decode(b"PNG\0rest"), Err(DecodeError::Magic));
    assert_eq!(decode(&bytes[..2]), Err(DecodeError::Magic));
    for cut in [HEADER_BYTES - 1, HEADER_BYTES + 3, bytes.len() - 1] {
        assert!(decode(&bytes[..cut]).is_err(), "cut at {cut}");
    }
    let mut longer = bytes.clone();
    longer.extend_from_slice(&[0; 4]);
    assert_eq!(decode(&longer), Err(DecodeError::Invalid("length")));
    // An unknown opcode.
    let ops = HEADER_BYTES + 12 + every_op().strings.len().next_multiple_of(4);
    let mut unknown = bytes.clone();
    unknown[ops] = 0x7F;
    assert_eq!(decode(&unknown), Err(DecodeError::Invalid("opcode")));
}

#[test]
fn a_string_reference_stays_inside_the_section() {
    let mut list = every_op();
    list.ops.push(Op::Text {
        x: 0.0,
        y: 0.0,
        align: Align::Left,
        face: Face::Sans,
        weight: 400,
        size: 12.0,
        paint: 0,
        text: Str { offset: 1, len: 2 },
        width: 0.0,
    });
    // Offset 1 is inside "P": fine. Inside a CJK character is not.
    assert!(decode(&encode(&list)).is_ok());
    let at = list.strings.find('季').unwrap_or(0) as u32 + 1;
    list.ops.pop();
    list.ops.push(Op::Text {
        x: 0.0,
        y: 0.0,
        align: Align::Left,
        face: Face::Sans,
        weight: 400,
        size: 12.0,
        paint: 0,
        text: Str { offset: at, len: 2 },
        width: 0.0,
    });
    assert_eq!(decode(&encode(&list)), Err(DecodeError::Invalid("string reference")));
}

#[test]
fn alignment_rules() {
    // Rects and box edges round; lines take the device pixel they start in.
    assert_eq!(snap(10.25, 0.0, 2.0), 21.0);
    assert_eq!(snap(10.0, 4.0, 1.5), 9.0);
    assert_eq!(line_pixel(39.0, 0.0, 1.0), 39.0);
    assert_eq!(line_pixel(39.0, 0.0, 2.0), 78.0);
    assert_eq!(line_pixel(3.0, 0.0, 1.5), 4.0);
    assert_eq!(line_pixel(0.1 + 0.2, 0.0, 10.0), 3.0, "a whole value with rounding error");
    assert_eq!(Align::Center.left(100.0, 40.0), 80.0);
    assert_eq!(Align::Right.left(100.0, 40.0), 60.0);
}
