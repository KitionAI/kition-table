//! The axis: what the editor needs of an arrangement to draw its rows, as one byte buffer the
//! engine's worker hands the page without a copy (`KitEngine.invoke` transfers typed arrays); and
//! the cells holding the search, asked for a run of rows at a time ([`Hits`]).
//!
//! Axis format v2, little-endian:
//!
//! ```text
//! 0   magic "KTAX"
//! 4   version u16 (2) | flags u16 (0)
//! 8   epoch u32       the arrangement this is; a newer one replaces it
//! 12  total u32       the table's records
//! 16  count u32       the records shown
//! 20  0 u32
//! 24  groups u32      group headers
//! 28  json u32        bytes of the group headers' JSON
//! 32  ids             f64 × count: the shown records' ids, in order (ids are below 2^53)
//!     groups          UTF-8 JSON: [{at, count, depth, collapsed, id, value}, …] in order
//! ```
//!
//! A group header sits before the record at index `at` and holds the `count` records from it;
//! headers of nested groups that start at the same record follow each other, outermost first.
//! The records of a collapsed group stay in `ids`: the editor leaves them out when it draws.
//!
//! The search hits are not in the axis: a search that most records hold would make it several
//! megabytes at 100,000 records. The editor asks for the rows it shows (`gridHits`), in the fields
//! it shows. Hits format v1, little-endian:
//!
//! ```text
//! 0   magic "KTHT"
//! 4   version u16 (1) | 0 u16
//! 8   epoch u32       the arrangement the rows are of
//! 12  from u32        the first row: an index into the axis's ids
//! 16  count u32       rows
//! 20  fields u32      the fields asked for
//! 24  bits            count × ceil(fields / 8) bytes: per row, bit (j % 8) of its byte j / 8 is
//!                     set when the j-th field asked for holds the search
//! ```
//!
//! The page and the engine ship together: another version is refused, never read.

use serde_json::{Value, json};

/// The axis format's version.
pub const VERSION: u16 = 2;
const MAGIC: &[u8; 4] = b"KTAX";
const HEADER: usize = 32;

/// The hits format's version.
pub const HITS_VERSION: u16 = 1;
const HITS_MAGIC: &[u8; 4] = b"KTHT";
const HITS_HEADER: usize = 24;

/// A group header of the axis.
#[derive(Clone, Debug, PartialEq)]
pub struct GroupHeader {
    /// The index in [`Axis::ids`] of its first record.
    pub at: u32,
    /// How many records it holds, its subgroups' included.
    pub count: u32,
    /// 0 for the outermost grouping.
    pub depth: u32,
    pub collapsed: bool,
    /// `g0:<value>::g1:<value>…`: its values and its parents', as JSON (the editor's group ids).
    pub id: String,
    /// Its records' value of the grouped field.
    pub value: Value,
}

/// An arrangement's rows.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Axis {
    pub epoch: u32,
    pub total: u32,
    pub ids: Vec<i64>,
    pub groups: Vec<GroupHeader>,
}

fn u32_of(value: usize) -> u32 {
    u32::try_from(value).unwrap_or(u32::MAX)
}

fn u16_at(bytes: &[u8], at: usize) -> Option<u16> {
    bytes.get(at..at + 2).and_then(|b| b.try_into().ok()).map(u16::from_le_bytes)
}

fn u32_at(bytes: &[u8], at: usize) -> Option<u32> {
    bytes.get(at..at + 4).and_then(|b| b.try_into().ok()).map(u32::from_le_bytes)
}

impl Axis {
    /// The axis in format v2.
    pub fn encode(&self) -> Vec<u8> {
        let groups: Vec<Value> = self
            .groups
            .iter()
            .map(|group| json!({ "at": group.at, "count": group.count, "depth": group.depth, "collapsed": group.collapsed, "id": group.id, "value": group.value }))
            .collect();
        let groups = if groups.is_empty() { Vec::new() } else { Value::Array(groups).to_string().into_bytes() };
        let mut bytes = Vec::with_capacity(HEADER + self.ids.len() * 8 + groups.len());
        bytes.extend_from_slice(MAGIC);
        bytes.extend_from_slice(&VERSION.to_le_bytes());
        bytes.extend_from_slice(&0u16.to_le_bytes());
        for value in [self.epoch, self.total, u32_of(self.ids.len()), 0, u32_of(self.groups.len()), u32_of(groups.len())] {
            bytes.extend_from_slice(&value.to_le_bytes());
        }
        for id in &self.ids {
            bytes.extend_from_slice(&(*id as f64).to_le_bytes());
        }
        bytes.extend_from_slice(&groups);
        bytes
    }

    /// An axis from format v2; an error for anything else.
    pub fn decode(bytes: &[u8]) -> Result<Self, String> {
        let f64_at = |at: usize| bytes.get(at..at + 8).and_then(|b| b.try_into().ok()).map(f64::from_le_bytes);
        let short = || "the axis is cut short".to_owned();
        if bytes.get(..4) != Some(MAGIC.as_slice()) {
            return Err("not an axis".into());
        }
        let version = u16_at(bytes, 4).ok_or_else(short)?;
        if version != VERSION {
            return Err(format!("axis version {version}, not {VERSION}"));
        }
        let field = |index: usize| u32_at(bytes, 8 + index * 4).ok_or_else(short);
        let (epoch, total, count, groups, json) = (field(0)?, field(1)?, field(2)? as usize, field(4)? as usize, field(5)? as usize);
        let ids = (0..count).map(|index| f64_at(HEADER + index * 8).map(|id| id as i64).ok_or_else(short)).collect::<Result<Vec<_>, _>>()?;
        let at = HEADER + count * 8;
        let text = bytes.get(at..at + json).ok_or_else(short)?;
        let parsed: Vec<Value> = if json == 0 { Vec::new() } else { serde_json::from_slice(text).map_err(|error| error.to_string())? };
        if parsed.len() != groups {
            return Err("the axis's group headers do not match their count".into());
        }
        let groups = parsed
            .into_iter()
            .map(|group| GroupHeader {
                at: group["at"].as_u64().map_or(0, |n| u32::try_from(n).unwrap_or(u32::MAX)),
                count: group["count"].as_u64().map_or(0, |n| u32::try_from(n).unwrap_or(u32::MAX)),
                depth: group["depth"].as_u64().map_or(0, |n| u32::try_from(n).unwrap_or(u32::MAX)),
                collapsed: group["collapsed"].as_bool().unwrap_or(false),
                id: group["id"].as_str().unwrap_or_default().to_owned(),
                value: group["value"].clone(),
            })
            .collect();
        Ok(Self { epoch, total, ids, groups })
    }
}

/// Which cells of a run of an arrangement's rows hold the search, in some of its fields.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Hits {
    pub epoch: u32,
    /// The first row: an index into the axis's ids.
    pub from: u32,
    pub count: u32,
    /// How many fields were asked for.
    pub fields: u32,
    /// Per row, `ceil(fields / 8)` bytes: bit `j % 8` of byte `j / 8` for the `j`-th field.
    pub bits: Vec<u8>,
}

impl Hits {
    /// No hits yet for `count` rows from `from`, in `fields` fields.
    pub fn new(epoch: u32, from: u32, count: u32, fields: u32) -> Self {
        let bits = vec![0; count as usize * (fields as usize).div_ceil(8)];
        Self { epoch, from, count, fields, bits }
    }

    fn stride(&self) -> usize {
        (self.fields as usize).div_ceil(8)
    }

    /// Marks the `column`-th field asked for in the `row`-th row of the run.
    pub fn set(&mut self, row: usize, column: usize) {
        let at = row * self.stride() + column / 8;
        if let Some(byte) = self.bits.get_mut(at) {
            *byte |= 1 << (column % 8);
        }
    }

    /// Whether the `column`-th field asked for holds the search in the `row`-th row of the run.
    pub fn holds(&self, row: usize, column: usize) -> bool {
        self.bits.get(row * self.stride() + column / 8).is_some_and(|byte| byte & (1 << (column % 8)) != 0)
    }

    /// The hits in format v1.
    pub fn encode(&self) -> Vec<u8> {
        let mut bytes = Vec::with_capacity(HITS_HEADER + self.bits.len());
        bytes.extend_from_slice(HITS_MAGIC);
        bytes.extend_from_slice(&HITS_VERSION.to_le_bytes());
        bytes.extend_from_slice(&0u16.to_le_bytes());
        for value in [self.epoch, self.from, self.count, self.fields] {
            bytes.extend_from_slice(&value.to_le_bytes());
        }
        bytes.extend_from_slice(&self.bits);
        bytes
    }

    /// Hits from format v1; an error for anything else.
    pub fn decode(bytes: &[u8]) -> Result<Self, String> {
        let short = || "the hits are cut short".to_owned();
        if bytes.get(..4) != Some(HITS_MAGIC.as_slice()) {
            return Err("not hits".into());
        }
        let version = u16_at(bytes, 4).ok_or_else(short)?;
        if version != HITS_VERSION {
            return Err(format!("hits version {version}, not {HITS_VERSION}"));
        }
        let field = |index: usize| u32_at(bytes, 8 + index * 4).ok_or_else(short);
        let mut hits = Self::new(field(0)?, field(1)?, field(2)?, field(3)?);
        let size = hits.bits.len();
        hits.bits.copy_from_slice(bytes.get(HITS_HEADER..HITS_HEADER + size).ok_or_else(short)?);
        Ok(hits)
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use super::*;

    #[test]
    fn an_axis_round_trips_and_another_version_is_refused() {
        let axis = Axis {
            epoch: 7,
            total: 9,
            ids: vec![3, 1, (1_i64 << 52) + 5],
            groups: vec![GroupHeader { at: 0, count: 3, depth: 0, collapsed: true, id: "g0:\"Done\"".into(), value: json!("Done") }],
        };
        let bytes = axis.encode();
        assert_eq!(&bytes[..4], b"KTAX");
        assert_eq!(Axis::decode(&bytes).unwrap(), axis);
        let mut other = bytes.clone();
        other[4] = 1;
        assert!(Axis::decode(&other).unwrap_err().contains("version 1"));
        assert!(Axis::decode(&bytes[..40]).is_err());
        let empty = Axis::default().encode();
        assert_eq!(empty.len(), 32);
        assert_eq!(Axis::decode(&empty).unwrap(), Axis::default());
    }

    #[test]
    fn hits_are_a_bitmask_per_row_and_round_trip() {
        let mut hits = Hits::new(4, 10, 3, 10);
        hits.set(0, 1);
        hits.set(2, 0);
        hits.set(2, 9);
        assert_eq!(hits.bits.len(), 6);
        assert!(hits.holds(0, 1) && hits.holds(2, 0) && hits.holds(2, 9));
        assert!(!hits.holds(1, 1) && !hits.holds(2, 8) && !hits.holds(9, 0));
        let bytes = hits.encode();
        assert_eq!((&bytes[..4], bytes.len()), (b"KTHT".as_slice(), 24 + 6));
        assert_eq!(Hits::decode(&bytes).unwrap(), hits);
        let mut other = bytes.clone();
        other[4] = 2;
        assert!(Hits::decode(&other).unwrap_err().contains("version 2"));
        assert!(Hits::decode(&bytes[..27]).is_err());
    }
}
