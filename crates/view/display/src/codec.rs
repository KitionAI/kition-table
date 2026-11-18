//! The byte format of a display list (see the crate's documentation): [`encode`] and [`decode`].

use crate::{Align, Axis, DisplayList, Face, Fit, HEADER_BYTES, HIT_BYTES, Header, Hit, HitKind, Key, Layer, Line, MAGIC, Op, Str, VERSION};

/// Why bytes are not a display list (or a resource table) of this version.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DecodeError {
    /// Not a display list or resource table (no `KTDL` or `KTRS` magic).
    Magic,
    /// A display list or resource table of another format version.
    Version(u16),
    /// The bytes end before a section or record does.
    Truncated,
    /// A field holds a value the format has no meaning for.
    Invalid(&'static str),
}

impl std::fmt::Display for DecodeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Magic => write!(f, "not a display list or resource table"),
            Self::Version(version) => write!(f, "format version {version}; this build reads version {VERSION}"),
            Self::Truncated => write!(f, "the bytes are cut short"),
            Self::Invalid(what) => write!(f, "the bytes hold an invalid {what}"),
        }
    }
}

impl std::error::Error for DecodeError {}

pub(crate) struct Writer(pub(crate) Vec<u8>);

impl Writer {
    pub(crate) fn u8(&mut self, v: u8) {
        self.0.push(v);
    }
    pub(crate) fn u16(&mut self, v: u16) {
        self.0.extend_from_slice(&v.to_le_bytes());
    }
    pub(crate) fn u32(&mut self, v: u32) {
        self.0.extend_from_slice(&v.to_le_bytes());
    }
    pub(crate) fn f32(&mut self, v: f32) {
        self.0.extend_from_slice(&v.to_le_bytes());
    }
    fn f32s(&mut self, values: &[f32]) {
        for v in values {
            self.f32(*v);
        }
    }
    /// An op's first word: the opcode, a byte argument and a paint (or zero).
    fn head(&mut self, op: u8, arg: u8, paint: u16) {
        self.u8(op);
        self.u8(arg);
        self.u16(paint);
    }
}

/// A section's length as its header field (the builders keep every count far below the limits).
fn count<T: TryFrom<usize> + Bounded>(items: usize) -> T {
    T::try_from(items).unwrap_or(T::MAX)
}

trait Bounded {
    const MAX: Self;
}

impl Bounded for u16 {
    const MAX: Self = u16::MAX;
}

impl Bounded for u32 {
    const MAX: Self = u32::MAX;
}

/// The list as bytes.
pub fn encode(list: &DisplayList) -> Vec<u8> {
    let mut w = Writer(Vec::with_capacity(list.bytes()));
    let header = &list.header;
    w.0.extend_from_slice(&MAGIC);
    w.u16(VERSION);
    w.u16(header.flags);
    w.u32(header.epoch);
    w.u16(header.theme);
    w.u16(header.resources);
    w.u8(header.key.layer as u8);
    w.u8(header.key.col_band);
    w.u16(0);
    w.u32(header.key.row_band);
    w.f32s(&[header.origin.0, header.origin.1, header.size.0, header.size.1]);
    w.u16(count::<u16>(list.palette.len()));
    w.u16(0);
    w.u32(count::<u32>(list.ops.len()));
    w.u32(count::<u32>(list.strings.len()));
    w.u32(count::<u32>(list.hits.len()));
    debug_assert_eq!(w.0.len(), HEADER_BYTES);
    for color in &list.palette {
        w.0.extend_from_slice(color);
    }
    w.0.extend_from_slice(list.strings.as_bytes());
    w.0.resize(w.0.len().next_multiple_of(4), 0);
    for op in &list.ops {
        let start = w.0.len();
        let code = op.opcode();
        match op {
            Op::Rect { x, y, w: width, h, paint, snap } => {
                w.head(code, u8::from(*snap), *paint);
                w.f32s(&[*x, *y, *width, *h]);
            }
            Op::RRect { x, y, w: width, h, r, paint } => {
                w.head(code, 0, *paint);
                w.f32s(&[*x, *y, *width, *h, *r]);
            }
            Op::RRectStroke { x, y, w: width, h, r, width: line, paint } => {
                w.head(code, 0, *paint);
                w.f32s(&[*x, *y, *width, *h, *r, *line]);
            }
            Op::Lines { axis, paint, lines } => {
                w.head(code, *axis as u8, *paint);
                w.u32(count::<u32>(lines.len()));
                for line in lines {
                    w.f32s(&[line.pos, line.from, line.to]);
                }
            }
            Op::Text { x, y, align, face, weight, size, paint, text, width } => {
                w.head(code, *align as u8, *paint);
                w.u8(*face as u8);
                w.u8(0);
                w.u16(*weight);
                w.f32s(&[*x, *y, *size, *width]);
                w.u32(text.offset);
                w.u16(text.len);
                w.u16(0);
            }
            Op::Tofu { x, y, w: width, h, paint } => {
                w.head(code, 0, *paint);
                w.f32s(&[*x, *y, *width, *h]);
            }
            Op::Emoji { atlas, x, y, size } => {
                w.head(code, 0, 0);
                w.u32(*atlas);
                w.f32s(&[*x, *y, *size]);
            }
            Op::Icon { icon, x, y, size, stroke, paint, solid } => {
                w.head(code, u8::from(*solid), *paint);
                w.u16(*icon);
                w.u16(0);
                w.f32s(&[*x, *y, *size, *stroke]);
            }
            Op::Image { key, x, y, w: width, h, fit, radius } => {
                w.head(code, *fit as u8, 0);
                w.u32(*key);
                w.f32s(&[*x, *y, *width, *h, *radius]);
            }
            Op::Shadow { x, y, w: width, h, r, blur, paint } => {
                w.head(code, 0, *paint);
                w.f32s(&[*x, *y, *width, *h, *r, *blur]);
            }
            Op::Clip { x, y, w: width, h } => {
                w.head(code, 0, 0);
                w.f32s(&[*x, *y, *width, *h]);
            }
            Op::Unclip => w.head(code, 0, 0),
        }
        debug_assert_eq!(w.0.len() - start, op.bytes());
    }
    for hit in &list.hits {
        w.u8(hit.kind as u8);
        w.u8(hit.part);
        w.u16(hit.index);
        w.u16(hit.col);
        w.u16(0);
        w.u32(hit.row);
        w.f32s(&[hit.x, hit.y, hit.w, hit.h]);
    }
    debug_assert_eq!(w.0.len(), list.bytes());
    w.0
}

pub(crate) struct Reader<'a> {
    pub(crate) bytes: &'a [u8],
    pub(crate) at: usize,
}

impl Reader<'_> {
    pub(crate) fn take<const N: usize>(&mut self) -> Result<[u8; N], DecodeError> {
        let end = self.at.checked_add(N).ok_or(DecodeError::Truncated)?;
        let slice = self.bytes.get(self.at..end).ok_or(DecodeError::Truncated)?;
        self.at = end;
        let mut out = [0u8; N];
        out.copy_from_slice(slice);
        Ok(out)
    }
    pub(crate) fn u8(&mut self) -> Result<u8, DecodeError> {
        Ok(self.take::<1>()?[0])
    }
    pub(crate) fn u16(&mut self) -> Result<u16, DecodeError> {
        Ok(u16::from_le_bytes(self.take()?))
    }
    pub(crate) fn u32(&mut self) -> Result<u32, DecodeError> {
        Ok(u32::from_le_bytes(self.take()?))
    }
    pub(crate) fn f32(&mut self) -> Result<f32, DecodeError> {
        Ok(f32::from_le_bytes(self.take()?))
    }
    fn f32s<const N: usize>(&mut self) -> Result<[f32; N], DecodeError> {
        let mut out = [0f32; N];
        for value in &mut out {
            *value = self.f32()?;
        }
        Ok(out)
    }
    pub(crate) fn skip(&mut self, n: usize) -> Result<(), DecodeError> {
        let end = self.at.checked_add(n).ok_or(DecodeError::Truncated)?;
        if end > self.bytes.len() {
            return Err(DecodeError::Truncated);
        }
        self.at = end;
        Ok(())
    }
}

/// Reads a list. Bytes of another version, or that are not a whole list, are refused.
pub fn decode(bytes: &[u8]) -> Result<DisplayList, DecodeError> {
    let mut r = Reader { bytes, at: 0 };
    if r.take::<4>().map_err(|_| DecodeError::Magic)? != MAGIC {
        return Err(DecodeError::Magic);
    }
    let version = r.u16()?;
    if version != VERSION {
        return Err(DecodeError::Version(version));
    }
    let flags = r.u16()?;
    let epoch = r.u32()?;
    let theme = r.u16()?;
    let resources = r.u16()?;
    let layer = Layer::from_u8(r.u8()?).ok_or(DecodeError::Invalid("layer"))?;
    let col_band = r.u8()?;
    r.skip(2)?;
    let row_band = r.u32()?;
    let [ox, oy, sw, sh] = r.f32s()?;
    let colors = usize::from(r.u16()?);
    r.skip(2)?;
    let ops = r.u32()? as usize;
    let string_bytes = r.u32()? as usize;
    let hits = r.u32()? as usize;
    let header = Header { flags, epoch, theme, resources, key: Key { layer, col_band, row_band }, origin: (ox, oy), size: (sw, sh) };

    let mut palette = Vec::with_capacity(colors.min(bytes.len() / 4));
    for _ in 0..colors {
        palette.push(r.take::<4>()?);
    }
    let start = r.at;
    r.skip(string_bytes)?;
    let strings = std::str::from_utf8(&bytes[start..start + string_bytes]).map_err(|_| DecodeError::Invalid("string"))?.to_owned();
    r.skip(string_bytes.next_multiple_of(4) - string_bytes)?;

    let mut list = DisplayList { header, palette, strings, ops: Vec::with_capacity(ops.min(bytes.len() / 4)), hits: Vec::new() };
    for _ in 0..ops {
        let code = r.u8()?;
        let arg = r.u8()?;
        let paint = r.u16()?;
        let op = match code {
            0x01 => {
                let [x, y, w, h] = r.f32s()?;
                Op::Rect { x, y, w, h, paint, snap: arg & 1 == 1 }
            }
            0x02 => {
                let [x, y, w, h, radius] = r.f32s()?;
                Op::RRect { x, y, w, h, r: radius, paint }
            }
            0x03 => {
                let [x, y, w, h, radius, width] = r.f32s()?;
                Op::RRectStroke { x, y, w, h, r: radius, width, paint }
            }
            0x04 => {
                let axis = Axis::from_u8(arg).ok_or(DecodeError::Invalid("axis"))?;
                let n = r.u32()? as usize;
                if n > (bytes.len() - r.at) / 12 {
                    return Err(DecodeError::Truncated);
                }
                let mut lines = Vec::with_capacity(n);
                for _ in 0..n {
                    let [pos, from, to] = r.f32s()?;
                    lines.push(Line { pos, from, to });
                }
                Op::Lines { axis, paint, lines }
            }
            0x05 => {
                let align = Align::from_u8(arg).ok_or(DecodeError::Invalid("align"))?;
                let face = Face::from_u8(r.u8()?).ok_or(DecodeError::Invalid("face"))?;
                r.skip(1)?;
                let weight = r.u16()?;
                let [x, y, size, width] = r.f32s()?;
                let text = Str { offset: r.u32()?, len: r.u16()? };
                r.skip(2)?;
                if (text.offset as usize).saturating_add(usize::from(text.len)) > list.strings.len()
                    || !list.strings.is_char_boundary(text.offset as usize)
                    || !list.strings.is_char_boundary(text.offset as usize + usize::from(text.len))
                {
                    return Err(DecodeError::Invalid("string reference"));
                }
                Op::Text { x, y, align, face, weight, size, paint, text, width }
            }
            0x06 => {
                let [x, y, w, h] = r.f32s()?;
                Op::Tofu { x, y, w, h, paint }
            }
            0x07 => {
                let atlas = r.u32()?;
                let [x, y, size] = r.f32s()?;
                Op::Emoji { atlas, x, y, size }
            }
            0x08 => {
                let icon = r.u16()?;
                r.skip(2)?;
                let [x, y, size, stroke] = r.f32s()?;
                Op::Icon { icon, x, y, size, stroke, paint, solid: arg & 1 == 1 }
            }
            0x09 => {
                let fit = Fit::from_u8(arg).ok_or(DecodeError::Invalid("fit"))?;
                let key = r.u32()?;
                let [x, y, w, h, radius] = r.f32s()?;
                Op::Image { key, x, y, w, h, fit, radius }
            }
            0x0A => {
                let [x, y, w, h, radius, blur] = r.f32s()?;
                Op::Shadow { x, y, w, h, r: radius, blur, paint }
            }
            0x0B => {
                let [x, y, w, h] = r.f32s()?;
                Op::Clip { x, y, w, h }
            }
            0x0C => Op::Unclip,
            _ => return Err(DecodeError::Invalid("opcode")),
        };
        list.ops.push(op);
    }
    if hits > (bytes.len() - r.at) / HIT_BYTES {
        return Err(DecodeError::Truncated);
    }
    list.hits.reserve(hits);
    for _ in 0..hits {
        let kind = HitKind::from_u8(r.u8()?).ok_or(DecodeError::Invalid("hit kind"))?;
        let part = r.u8()?;
        let index = r.u16()?;
        let col = r.u16()?;
        r.skip(2)?;
        let row = r.u32()?;
        let [x, y, w, h] = r.f32s()?;
        list.hits.push(Hit { kind, part, index, col, row, x, y, w, h });
    }
    if r.at != bytes.len() {
        return Err(DecodeError::Invalid("length"));
    }
    Ok(list)
}
