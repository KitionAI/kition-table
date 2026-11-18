//! The byte format of a resource table (see the crate's documentation, "Resource table"):
//! [`encode_resources`] and [`decode_resources`].

use crate::codec::{Reader, Writer};
use crate::raster::{IconStyle, Tile};
use crate::resources::{Entry, ResourceTable};
use crate::{DecodeError, RESOURCE_MAGIC, RESOURCE_VERSION};

/// An entry's kind byte.
const ICON: u8 = 1;
const EMOJI: u8 = 2;
const IMAGE: u8 = 3;

fn length(bytes: usize) -> u32 {
    u32::try_from(bytes).unwrap_or(u32::MAX)
}

/// A tile: width u32 | height u32 | left i32 | top i32 | straight RGBA8.
fn tile_bytes(p: &mut Writer, tile: &Tile) {
    p.u32(tile.width);
    p.u32(tile.height);
    p.0.extend_from_slice(&tile.left.to_le_bytes());
    p.0.extend_from_slice(&tile.top.to_le_bytes());
    p.0.extend_from_slice(&tile.rgba);
}

/// The table as bytes.
pub fn encode_resources(table: &ResourceTable) -> Vec<u8> {
    let mut w = Writer(Vec::new());
    w.0.extend_from_slice(&RESOURCE_MAGIC);
    w.u16(RESOURCE_VERSION);
    w.u16(table.version);
    w.u32(length(table.entries.len()));
    for entry in &table.entries {
        let mut p = Writer(Vec::new());
        let kind = match entry {
            Entry::Icon { id, style, dpr, tile } => {
                p.u16(*id);
                p.u8(u8::from(style.solid));
                p.u8(0);
                p.f32(style.size);
                p.f32(style.stroke);
                p.f32(*dpr);
                p.0.extend_from_slice(&style.paint);
                tile_bytes(&mut p, tile);
                ICON
            }
            Entry::Emoji { atlas, size, dpr, tile } => {
                p.u32(*atlas);
                p.f32(*size);
                p.f32(*dpr);
                tile_bytes(&mut p, tile);
                EMOJI
            }
            Entry::Image { key, width, height, source } => {
                p.u32(*key);
                p.u32(*width);
                p.u32(*height);
                p.u32(length(source.len()));
                p.0.extend_from_slice(source.as_bytes());
                IMAGE
            }
        };
        w.u8(kind);
        w.u8(0);
        w.u16(0);
        w.u32(length(p.0.len()));
        w.0.extend_from_slice(&p.0);
        w.0.resize(w.0.len().next_multiple_of(4), 0);
    }
    w.0
}

impl Reader<'_> {
    fn bytes(&mut self, n: usize) -> Result<&[u8], DecodeError> {
        let start = self.at;
        self.skip(n)?;
        Ok(&self.bytes[start..start + n])
    }

    fn text(&mut self, n: usize) -> Result<String, DecodeError> {
        std::str::from_utf8(self.bytes(n)?).map(str::to_owned).map_err(|_| DecodeError::Invalid("string"))
    }

    fn i32(&mut self) -> Result<i32, DecodeError> {
        Ok(i32::from_le_bytes(self.take()?))
    }

    fn tile(&mut self) -> Result<Tile, DecodeError> {
        let (width, height) = (self.u32()?, self.u32()?);
        let (left, top) = (self.i32()?, self.i32()?);
        let n = (width as usize).checked_mul(height as usize).and_then(|n| n.checked_mul(4)).ok_or(DecodeError::Invalid("tile size"))?;
        let rgba = self.bytes(n)?.to_vec();
        Ok(Tile { width, height, left, top, rgba })
    }
}

/// Reads one entry's payload (exactly `payload.len()` bytes).
fn entry(kind: u8, payload: &[u8]) -> Result<Entry, DecodeError> {
    let mut r = Reader { bytes: payload, at: 0 };
    let entry = match kind {
        ICON => {
            let id = r.u16()?;
            let solid = r.u8()? != 0;
            r.skip(1)?;
            let (size, stroke, dpr) = (r.f32()?, r.f32()?, r.f32()?);
            let paint = r.take::<4>()?;
            Entry::Icon { id, style: IconStyle { size, stroke, solid, paint }, dpr, tile: r.tile()? }
        }
        EMOJI => {
            let atlas = r.u32()?;
            let (size, dpr) = (r.f32()?, r.f32()?);
            Entry::Emoji { atlas, size, dpr, tile: r.tile()? }
        }
        IMAGE => {
            let key = r.u32()?;
            let (width, height) = (r.u32()?, r.u32()?);
            let n = r.u32()? as usize;
            Entry::Image { key, width, height, source: r.text(n)? }
        }
        _ => return Err(DecodeError::Invalid("resource kind")),
    };
    if r.at != payload.len() {
        return Err(DecodeError::Invalid("entry length"));
    }
    Ok(entry)
}

/// Reads a resource table. Bytes of another version, or that are not a whole table, are refused.
pub fn decode_resources(bytes: &[u8]) -> Result<ResourceTable, DecodeError> {
    let mut r = Reader { bytes, at: 0 };
    if r.take::<4>().map_err(|_| DecodeError::Magic)? != RESOURCE_MAGIC {
        return Err(DecodeError::Magic);
    }
    let format = r.u16()?;
    if format != RESOURCE_VERSION {
        return Err(DecodeError::Version(format));
    }
    let version = r.u16()?;
    let count = r.u32()? as usize;
    if count > (bytes.len() - r.at) / 8 {
        return Err(DecodeError::Truncated);
    }
    let mut entries = Vec::with_capacity(count);
    for _ in 0..count {
        let kind = r.u8()?;
        r.skip(3)?;
        let n = r.u32()? as usize;
        let start = r.at;
        r.skip(n)?;
        entries.push(entry(kind, &bytes[start..start + n])?);
        r.skip(n.next_multiple_of(4) - n)?;
    }
    if r.at != bytes.len() {
        return Err(DecodeError::Invalid("length"));
    }
    Ok(ResourceTable { version, entries })
}
