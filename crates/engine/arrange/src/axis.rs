//! The axis: what the editor needs of an arrangement to draw its rows, as one byte buffer the
//! engine's worker hands the page without a copy (`KitEngine.invoke` transfers typed arrays).
//!
//! Format v1, little-endian:
//!
//! ```text
//! 0   magic "KTAX"
//! 4   version u16 (1) | flags u16 (0)
//! 8   epoch u32       the arrangement this is; a newer one replaces it
//! 12  total u32       the table's records
//! 16  count u32       the records shown
//! 20  hits u32        search hits
//! 24  groups u32      group headers
//! 28  json u32        bytes of the group headers' JSON
//! 32  ids             f64 × count: the shown records' ids, in order (ids are below 2^53)
//!     hits            16 B each: row u32 (an index into ids), 0 u32, field id f64
//!     groups          UTF-8 JSON: [{at, count, depth, collapsed, id, value}, …] in order
//! ```
//!
//! A group header sits before the record at index `at` and holds the `count` records from it;
//! headers of nested groups that start at the same record follow each other, outermost first.
//! The records of a collapsed group stay in `ids`: the editor leaves them out when it draws.
//! The page and the engine ship together: another version is refused, never read.

use serde_json::{Value, json};

/// The axis format's version.
pub const VERSION: u16 = 1;
const MAGIC: &[u8; 4] = b"KTAX";
const HEADER: usize = 32;

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
    /// `(index in ids, field id)` of each cell whose text holds the search.
    pub hits: Vec<(u32, i64)>,
    pub groups: Vec<GroupHeader>,
}

fn u32_of(value: usize) -> u32 {
    u32::try_from(value).unwrap_or(u32::MAX)
}

impl Axis {
    /// The axis in format v1.
    pub fn encode(&self) -> Vec<u8> {
        let groups: Vec<Value> = self
            .groups
            .iter()
            .map(|group| json!({ "at": group.at, "count": group.count, "depth": group.depth, "collapsed": group.collapsed, "id": group.id, "value": group.value }))
            .collect();
        let groups = if groups.is_empty() { Vec::new() } else { Value::Array(groups).to_string().into_bytes() };
        let mut bytes = Vec::with_capacity(HEADER + self.ids.len() * 8 + self.hits.len() * 16 + groups.len());
        bytes.extend_from_slice(MAGIC);
        bytes.extend_from_slice(&VERSION.to_le_bytes());
        bytes.extend_from_slice(&0u16.to_le_bytes());
        for value in [self.epoch, self.total, u32_of(self.ids.len()), u32_of(self.hits.len()), u32_of(self.groups.len()), u32_of(groups.len())] {
            bytes.extend_from_slice(&value.to_le_bytes());
        }
        for id in &self.ids {
            bytes.extend_from_slice(&(*id as f64).to_le_bytes());
        }
        for (row, field) in &self.hits {
            bytes.extend_from_slice(&row.to_le_bytes());
            bytes.extend_from_slice(&0u32.to_le_bytes());
            bytes.extend_from_slice(&(*field as f64).to_le_bytes());
        }
        bytes.extend_from_slice(&groups);
        bytes
    }

    /// An axis from format v1; an error for anything else.
    pub fn decode(bytes: &[u8]) -> Result<Self, String> {
        let u16_at = |at: usize| bytes.get(at..at + 2).and_then(|b| b.try_into().ok()).map(u16::from_le_bytes);
        let u32_at = |at: usize| bytes.get(at..at + 4).and_then(|b| b.try_into().ok()).map(u32::from_le_bytes);
        let f64_at = |at: usize| bytes.get(at..at + 8).and_then(|b| b.try_into().ok()).map(f64::from_le_bytes);
        let short = || "the axis is cut short".to_owned();
        if bytes.get(..4) != Some(MAGIC.as_slice()) {
            return Err("not an axis".into());
        }
        let version = u16_at(4).ok_or_else(short)?;
        if version != VERSION {
            return Err(format!("axis version {version}, not {VERSION}"));
        }
        let field = |index: usize| u32_at(8 + index * 4).ok_or_else(short);
        let (epoch, total, count, hits, groups, json) =
            (field(0)?, field(1)?, field(2)? as usize, field(3)? as usize, field(4)? as usize, field(5)? as usize);
        let ids = (0..count).map(|index| f64_at(HEADER + index * 8).map(|id| id as i64).ok_or_else(short)).collect::<Result<Vec<_>, _>>()?;
        let at = HEADER + count * 8;
        let hits = (0..hits)
            .map(|index| Some((u32_at(at + index * 16)?, f64_at(at + index * 16 + 8)? as i64)))
            .collect::<Option<Vec<_>>>()
            .ok_or_else(short)?;
        let at = at + hits.len() * 16;
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
        Ok(Self { epoch, total, ids, hits, groups })
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
            hits: vec![(0, 12), (2, 40)],
            groups: vec![GroupHeader { at: 0, count: 3, depth: 0, collapsed: true, id: "g0:\"Done\"".into(), value: json!("Done") }],
        };
        let bytes = axis.encode();
        assert_eq!(&bytes[..4], b"KTAX");
        assert_eq!(Axis::decode(&bytes).unwrap(), axis);
        let mut other = bytes.clone();
        other[4] = 2;
        assert!(Axis::decode(&other).unwrap_err().contains("version 2"));
        assert!(Axis::decode(&bytes[..40]).is_err());
        let empty = Axis::default().encode();
        assert_eq!(empty.len(), 32);
        assert_eq!(Axis::decode(&empty).unwrap(), Axis::default());
    }
}
