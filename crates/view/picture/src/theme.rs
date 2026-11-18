//! The editor's light theme, as the picture's bars and cards draw them (`web/src/styles/theme.css`);
//! the grid's colors are `kition-table-grid`'s.

use crate::scene::{Rgba, hex};

pub const PAPER: Rgba = hex(0xFFFFFF);
/// `--foreground`.
pub const FOREGROUND: Rgba = hex(0x1A1A1A);
/// `--muted-foreground`.
pub const MUTED: Rgba = hex(0x79776F);
/// `--muted`.
pub const MUTED_BG: Rgba = hex(0xF6F5F4);
/// `--border`.
pub const BORDER: Rgba = hex(0xE6E3E0);
/// `--accent`: the active table tab.
pub const ACCENT: Rgba = hex(0xEEF6F5);
/// `--primary`: the kit brand (kit.json `ui.brand.light`).
pub const PRIMARY: Rgba = hex(0x0C8375);
/// `--primary` at 10 % on white: an active view tab, the calendar's dates.
pub const PRIMARY_SOFT: Rgba = hex(0xE7F3F1);
/// `--primary` at 20 % on white.
pub const PRIMARY_LINE: Rgba = hex(0xCEE6E3);

/// The grid: cell text, lines, the header row, row numbers, icons.
pub const CELL_TEXT: Rgba = hex(0x18181B);
pub const CELL_LINE: Rgba = hex(0xE4E4E7);
pub const ROW_NUMBER: Rgba = hex(0x71717A);
pub const ICON: Rgba = hex(0x6B7280);
/// Links between records: the highlight text colour.
pub const LINK: Rgba = hex(0x8B5CF6);
/// URLs.
pub const URL: Rgba = hex(0x2563EB);
/// A user chip and its avatar.
pub const OPTION_BG: Rgba = hex(0xE5E7EB);
pub const AVATAR_BG: Rgba = hex(0xF3F4F6);
/// Rating stars.
pub const STAR: Rgba = hex(0xF59E0B);
pub const STAR_OFF: Rgba = hex(0xE4E4E7);
/// A kanban column (`--secondary` at 70 %).
pub const COLUMN_BG: Rgba = hex(0xF7F6F5);
/// The chart colours of dashboards.
pub const CHART: [Rgba; 6] = [hex(0x5645D4), hex(0x2A9D99), hex(0xDD5B00), hex(0x7B3FF2), hex(0x1AAE39), hex(0xFF64C8)];

/// Font sizes and weights.
pub const SMALL: f64 = 12.0;
pub const BODY: f64 = 13.0;
pub const LARGE: f64 = 14.0;
pub const REGULAR: u16 = 400;
pub const MEDIUM: u16 = 500;
pub const SEMIBOLD: u16 = 600;
