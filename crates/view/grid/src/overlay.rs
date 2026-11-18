//! How the editor's overlay draws (grid plan section 3.1): the selection, the active cell, the
//! hovered row and column header, and the row head's controls over a hovered, active or checked
//! row. The overlay is editor state and never part of a picture, so the page draws it, but with
//! these colors and sizes: the page does not pick its own. Where each control sits is the row
//! head band's hit records (one slot per control, as tall as the row); a control is drawn
//! centred in its slot.

use crate::theme::{Mode, Rgba, Theme, alpha};

/// A row head control's icon, and a checkbox's box, in CSS pixels.
pub const CONTROL: f32 = 16.0;
/// A checkbox's corner radius and border width.
pub const CHECKBOX_RADIUS: f32 = 4.0;
pub const CHECKBOX_BORDER: f32 = 1.0;
/// The check mark's size inside a checked box.
pub const CHECK_MARK: f32 = 12.0;
/// An icon's stroke, in its 24-unit grid (the cells' icons use the same).
pub const ICON_STROKE: f32 = 1.5;
/// The active cell's outline and a range's outline.
pub const ACTIVE_WIDTH: f32 = 2.0;
pub const RANGE_WIDTH: f32 = 1.0;
/// The Tessera icons of the row head's controls.
pub const DRAG_ICON: &str = "grip-vertical";
pub const EXPAND_ICON: &str = "maximize-2";
pub const CHECK_ICON: &str = "check";

/// The overlay's colors for a theme.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Overlay {
    /// Selected cells, rows and columns.
    pub selection: Rgba,
    /// A range's outline and the active cell's: the brand.
    pub border: Rgba,
    /// The hovered row and the hovered column header.
    pub hover: Rgba,
    /// Behind the row head's controls (they replace the row number).
    pub row_head: Rgba,
    /// The controls' icons and an unchecked box's border.
    pub control: Rgba,
    /// A checked box's fill and border: the brand.
    pub check: Rgba,
    /// The mark on a checked box.
    pub check_mark: Rgba,
}

impl Overlay {
    pub fn of(theme: &Theme) -> Self {
        let hover = match theme.mode {
            Mode::Light => [0, 0, 0, 10],
            Mode::Dark => [255, 255, 255, 14],
        };
        Self {
            selection: alpha(theme.primary, 26),
            border: theme.primary,
            hover,
            row_head: theme.paper,
            control: theme.muted,
            check: theme.primary,
            check_mark: theme.on_primary,
        }
    }
}
