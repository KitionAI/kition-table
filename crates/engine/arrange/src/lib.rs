//! # kition-table-arrange
//!
//! The Table kit's view model: which records a view shows and in what order. A view's filter,
//! its groups and sorts ([`Arrangement`]), and the editor's search, applied to a table's records
//! ([`Arranged`]), give the record order and the group headers ([`Axis`]) the editor draws, kept
//! up to date edit by edit. It is the kit's one arrangement: the editor's grid, kanban, gallery
//! and calendar read the order from it through the engine (`gridOpen`, `gridSync`), and the
//! agent's `read` arranges with it.
//!
//! Ordering compares each field's text naturally (runs of digits as numbers, case ignored,
//! [`value::natural_cmp`]); empty cells come first. Records that tie keep the table's order.

pub mod arranged;
pub mod axis;
pub mod filter;
mod rows;
mod settings;
pub mod stats;
pub mod value;

pub use arranged::{Arranged, RELOCATE_LIMIT, group_id};
pub use axis::{Axis, GroupHeader};
pub use rows::{Order, Row};
pub use settings::{Arrangement, view_records};
pub use stats::{Stats, Stopwatch, Timing};
