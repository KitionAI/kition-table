//! `kition-table-grid`: the Table grid's layout, as a display list (`kition-table-display`).
//!
//! The grid is laid out once, in Rust, for every player: the headless picture rasterizes the list
//! (`kition-table-picture`), and the editor's page will play the same list on a Canvas. Geometry
//! is the editor's (a row head of its controls, a 32 px header row, rows of 32, 40, 56 or 88 px,
//! 48 px group headers, 32 px add rows, a 100 px add-field column, columns 200 px unless the view
//! sets a width); colors are the kit's tokens ([`theme`]). Text is shaped by `kition-table-fonts`
//! into runs of one face each, cut and wrapped here ([`text`]), so a player never measures.
//!
//! The records' arrangement (filter, sort, groups) is the caller's: [`View`] takes the records in
//! order and the fields they are grouped by.

mod cells;
pub mod format;
mod layout;
pub mod text;
pub mod theme;

pub use cells::{Area, Out, cell, chip, field_icon};
pub use layout::{
    APPEND_COLUMN, APPEND_ROW, Column, DEFAULT_WIDTH, Focus, GROUP_HEADER, Geometry, Grid, HEADER, ROW_CONTROL, Row, View, geometry, height, lay_out,
    row_height, rows, width,
};

#[cfg(test)]
mod tests;
