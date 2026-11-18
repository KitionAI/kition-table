//! The byte format of a resource table (see the crate's documentation, "Resource table"):
//! [`encode_resources`] and [`decode_resources`].

use crate::codec::{Reader, Writer};
use crate::raster::Tile;
use crate::resources::{Entry, ResourceTable};
use crate::{DecodeError, RESOURCE_MAGIC, RESOURCE_VERSION};

/// An entry's kind byte.
const ICON: u8 = 1;
const EMOJI: u8 = 2;
const IMAGE: u8 = 3;

fn length(bytes: usize) -> u32 {
    u32::try_from(bytes).unwrap_or(u32::MAX)
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
            Entry::Icon { id, grid, paths, fills } => {
                p.u16(*id);
                p.u16(u16::try_from(paths.len()).unwrap_or(u16::MAX));
                p.u16(u16::try_from(fills.len()).unwrap_or(u16::MAX));
                p.u16(0);
                p.f32(*grid);
                for path in paths.iter().chain(fills) {
                    p.u32(length(path.len()));
                }
                for path in paths.iter().chain(fills) {
                    p.0.extend_from_slice(path.as_bytes());
                }
                ICON
            }
            Entry::Emoji { atlas, size, dpr, tile } => {
                p.u32(*atlas);
                p.f32(*size);
                p.f32(*dpr);
                p.u32(tile.width);
                p.u32(tile.height);
                p.0.extend_from_slice(&tile.left.to_le_bytes());
                p.0.extend_from_slice(&tile.top.to_le_bytes());
                p.0.extend_from_slice(&tile.rgba);
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
}

/// Reads one entry's payload (exactly `payload.len()` bytes).
fn entry(kind: u8, payload: &[u8]) -> Result<Entry, DecodeError> {
    let mut r = Reader { bytes: payload, at: 0 };
    let entry = match kind {
        ICON => {
            let id = r.u16()?;
            let (paths, fills) = (usize::from(r.u16()?), usize::from(r.u16()?));
            r.skip(2)?;
            let grid = r.f32()?;
            if (paths + fills) * 4 > payload.len() - r.at {
                return Err(DecodeError::Truncated);
            }
            let mut lengths = Vec::with_capacity(paths + fills);
            for _ in 0..paths + fills {
                lengths.push(r.u32()? as usize);
            }
            let mut data = Vec::with_capacity(lengths.len());
            for n in lengths {
                data.push(r.text(n)?);
            }
            let fills = data.split_off(paths);
            Entry::Icon { id, grid, paths: data, fills }
        }
        EMOJI => {
            let atlas = r.u32()?;
            let (size, dpr) = (r.f32()?, r.f32()?);
            let (width, height) = (r.u32()?, r.u32()?);
            let (left, top) = (r.i32()?, r.i32()?);
            let n = (width as usize).checked_mul(height as usize).and_then(|n| n.checked_mul(4)).ok_or(DecodeError::Invalid("tile size"))?;
            let rgba = r.bytes(n)?.to_vec();
            Entry::Emoji { atlas, size, dpr, tile: Tile { width, height, left, top, rgba } }
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
