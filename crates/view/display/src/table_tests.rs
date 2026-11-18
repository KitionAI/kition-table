//! The resource table's bytes: every entry kind round trips, the bytes sit where the format
//! says, and bytes that are not a whole table of this version are refused.

use super::*;

fn every_entry() -> ResourceTable {
    let mut resources = Resources::default();
    let star = resources.icon("star").expect("Tessera has a star");
    let clip = resources.icon("paperclip").expect("Tessera has a paperclip");
    let photo = resources.image("kition-blob:0123abcd");
    resources.set_image_size(photo, 640, 480);
    let unknown = resources.image("https://example.com/a picture.png");
    let mut entries: Vec<Entry> = [(star, true, 2.0), (clip, false, 1.5)]
        .iter()
        .filter_map(|(id, solid, dpr)| {
            resources.icon_entry(*id, IconStyle { size: 16.0, stroke: 1.5, solid: *solid, paint: [245, 158, 11, 255] }, *dpr)
        })
        .collect();
    entries.push(Entry::Emoji { atlas: 3, size: 13.0, dpr: 1.5, tile: Tile { width: 2, height: 3, left: 0, top: -21, rgba: (0..24).collect() } });
    entries.extend([photo, unknown].iter().filter_map(|key| resources.image_entry(*key)));
    ResourceTable { version: resources.version, entries }
}

#[test]
fn a_table_round_trips_through_its_bytes() {
    let table = every_entry();
    assert_eq!(table.entries.len(), 5);
    let bytes = encode_resources(&table);
    assert_eq!(bytes.len() % 4, 0, "4-byte aligned");
    assert_eq!(&bytes[..4], b"KTRS");
    assert_eq!(u16::from_le_bytes([bytes[4], bytes[5]]), RESOURCE_VERSION);
    assert_eq!(u16::from_le_bytes([bytes[6], bytes[7]]), table.version);
    assert_eq!(u32::from_le_bytes([bytes[8], bytes[9], bytes[10], bytes[11]]), 5);
    assert_eq!(decode_resources(&bytes), Ok(table.clone()));
    assert_eq!(decode_resources(&encode_resources(&ResourceTable::default())), Ok(ResourceTable::default()));
}

#[test]
fn entries_sit_where_the_format_says() {
    let mut resources = Resources::default();
    let star = resources.icon("star").expect("star");
    let style = IconStyle { size: 16.0, stroke: 1.5, solid: true, paint: [1, 2, 3, 255] };
    let table = ResourceTable { version: 9, entries: vec![resources.icon_entry(star, style, 2.0).expect("entry")] };
    let tile = icon_tile(&resources, star, style, 2.0).expect("a tile");
    let bytes = encode_resources(&table);
    // The entry's head: kind 1, length of the payload.
    assert_eq!(bytes[12], 1);
    let length = u32::from_le_bytes([bytes[16], bytes[17], bytes[18], bytes[19]]) as usize;
    let payload = &bytes[20..20 + length];
    assert_eq!(u16::from_le_bytes([payload[0], payload[1]]), star);
    assert_eq!(payload[2], 1, "solid");
    let floats: Vec<f32> = payload[4..16].as_chunks::<4>().0.iter().map(|word| f32::from_le_bytes(*word)).collect();
    assert_eq!(floats, [16.0, 1.5, 2.0], "size, stroke, dpr");
    assert_eq!(&payload[16..20], &[1, 2, 3, 255], "the color");
    let words: Vec<u32> = payload[20..36].as_chunks::<4>().0.iter().map(|word| u32::from_le_bytes(*word)).collect();
    assert_eq!(words, [tile.width, tile.height, tile.left as u32, tile.top as u32]);
    assert_eq!(&payload[36..], tile.rgba.as_slice());
    assert_eq!(bytes.len(), 20 + length.next_multiple_of(4));

    // An image: key, size, url.
    let key = resources.image("kition-blob:ff");
    resources.set_image_size(key, 3, 4);
    let bytes = encode_resources(&ResourceTable { version: 1, entries: vec![resources.image_entry(key).expect("entry")] });
    assert_eq!(bytes[12], 3);
    let words: Vec<u32> = bytes[20..36].as_chunks::<4>().0.iter().map(|word| u32::from_le_bytes(*word)).collect();
    assert_eq!(words, [key, 3, 4, 14]);
    assert_eq!(&bytes[36..50], b"kition-blob:ff");
}

#[test]
fn an_emoji_entry_is_the_rasterizers_tile() {
    assert!(raster_tests::fonts());
    let runs = kition_table_fonts::shape("\u{1F600}", kition_table_fonts::Family::Sans, 400, 13.0);
    let mut resources = Resources::default();
    let atlas = resources.emoji(runs[0].face, runs[0].glyphs[0].id);
    let Some(Entry::Emoji { tile, dpr, .. }) = resources.emoji_entry(atlas, 13.0, 2.0) else { panic!("an emoji entry") };
    assert_eq!(Some(tile), emoji_tile(&resources, atlas, 13.0, 2.0));
    assert_eq!(dpr, 2.0);
    assert_eq!(resources.emoji_entry(atlas + 1, 13.0, 2.0), None);
}

#[test]
fn image_keys_are_stable_and_sizes_known_or_zero() {
    let mut resources = Resources::default();
    let before = resources.version;
    let a = resources.image("kition-blob:aa");
    let b = resources.image("kition-blob:bb");
    assert_ne!(a, b);
    assert_eq!(resources.image("kition-blob:aa"), a, "one key per url");
    assert!(resources.version != before, "adding an image bumps the version");
    assert_eq!(resources.image_size(a), None);
    assert_eq!(resources.image_entry(a), Some(Entry::Image { key: a, width: 0, height: 0, source: "kition-blob:aa".into() }));
    let pixels = vello_cpu::Pixmap::from_parts(vec![vello_cpu::peniko::color::PremulRgba8 { r: 0, g: 0, b: 0, a: 255 }; 6], 3, 2);
    resources.set_image(b, pixels);
    assert_eq!(resources.image_size(b), Some((3, 2)), "a decoded picture's size");
    resources.set_image_size(a, 1200, 800);
    assert_eq!(resources.image_size(a), Some((1200, 800)));
    assert_eq!(resources.image_keys().collect::<Vec<_>>(), [a, b]);
}

#[test]
fn bytes_that_are_not_a_whole_table_are_refused() {
    let bytes = encode_resources(&every_entry());
    assert_eq!(decode_resources(b"KTDL\x01\x00"), Err(DecodeError::Magic));
    let mut other = bytes.clone();
    other[4] = 2;
    assert_eq!(decode_resources(&other), Err(DecodeError::Version(2)));
    for cut in [5, 12, 13, 20, bytes.len() - 4] {
        assert!(decode_resources(&bytes[..cut]).is_err(), "cut at {cut}");
    }
    let mut long = bytes.clone();
    long.extend_from_slice(&[0; 4]);
    assert_eq!(decode_resources(&long), Err(DecodeError::Invalid("length")));
    let mut kind = bytes.clone();
    kind[12] = 9;
    assert_eq!(decode_resources(&kind), Err(DecodeError::Invalid("resource kind")));
    // An emoji tile larger than its entry.
    let tile = encode_resources(&ResourceTable {
        version: 0,
        entries: vec![Entry::Emoji { atlas: 0, size: 1.0, dpr: 1.0, tile: Tile { width: 1, height: 1, left: 0, top: 0, rgba: vec![1, 2, 3, 4] } }],
    });
    let mut big = tile.clone();
    big[32] = 2; // width
    assert_eq!(decode_resources(&big), Err(DecodeError::Truncated));
    // An image url that is not UTF-8.
    let mut url = encode_resources(&ResourceTable { version: 0, entries: vec![Entry::Image { key: 0, width: 0, height: 0, source: "ab".into() }] });
    url[36] = 0xFF;
    assert_eq!(decode_resources(&url), Err(DecodeError::Invalid("string")));
}
