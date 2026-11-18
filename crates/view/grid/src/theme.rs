//! The grid's colors, from the kit's own tokens (`web/src/styles/theme.css`, light and `.dark`),
//! and its fixed sizes. The brand is `kit.json`'s `ui.brand`; a host may hand another.

/// Straight (not premultiplied) RGBA.
pub type Rgba = [u8; 4];

/// A colour from `0xRRGGBB`.
pub const fn hex(rgb: u32) -> Rgba {
    [(rgb >> 16) as u8, (rgb >> 8) as u8, rgb as u8, 255]
}

/// `color` at `alpha` (0..=255).
pub const fn alpha(color: Rgba, alpha: u8) -> Rgba {
    [color[0], color[1], color[2], alpha]
}

/// Light or dark.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum Mode {
    #[default]
    Light,
    Dark,
}

/// The colors the grid draws with.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Theme {
    pub mode: Mode,
    /// `--background`: cells, the add row.
    pub paper: Rgba,
    /// `--foreground`: cell text, column titles.
    pub text: Rgba,
    /// `--muted-foreground`: row numbers, icons, group labels.
    pub muted: Rgba,
    /// `--border`: cell lines.
    pub line: Rgba,
    /// `--hairline-strong`: the frozen columns' divider.
    pub divider: Rgba,
    /// `--surface-soft`: the header row, the add column.
    pub header: Rgba,
    /// Group header rows, outermost first: `--tint-gray`, `--muted`, `--surface-soft`; a view
    /// grouped by n fields uses the last n.
    pub groups: [Rgba; 3],
    /// `--secondary`: a person's chip.
    pub chip: Rgba,
    /// `--tint-gray`: an avatar.
    pub avatar: Rgba,
    /// `--primary`: the brand (checked boxes, links, the focus outline).
    pub primary: Rgba,
    /// `--primary-foreground`: what is drawn on the brand.
    pub on_primary: Rgba,
    /// `--accent`: a button's background.
    pub accent: Rgba,
    /// `--info`: URLs.
    pub url: Rgba,
    /// A rating's filled symbols.
    pub rating: Rgba,
}

impl Theme {
    pub const LIGHT: Theme = Theme {
        mode: Mode::Light,
        paper: hex(0xFFFFFF),
        text: hex(0x1A1A1A),
        muted: hex(0x797772),
        line: hex(0xE6E3E0),
        divider: hex(0xCDC8C1),
        header: hex(0xF8F7F6),
        groups: [hex(0xEFEDEC), hex(0xF6F5F4), hex(0xF8F7F6)],
        chip: hex(0xF6F5F4),
        avatar: hex(0xEFEDEC),
        primary: hex(0x0C8375),
        on_primary: hex(0xFFFFFF),
        accent: hex(0xEEF6F5),
        url: hex(0x2178C0),
        rating: hex(0xF59E0B),
    };

    pub const DARK: Theme = Theme {
        mode: Mode::Dark,
        paper: hex(0x1B1D23),
        text: hex(0xEEF0F2),
        muted: hex(0xABB1BA),
        line: hex(0x353941),
        divider: hex(0x4C515D),
        header: hex(0x181B20),
        groups: [hex(0x2A2927), hex(0x292C33), hex(0x22252B)],
        chip: hex(0x292C33),
        avatar: hex(0x353941),
        primary: hex(0x3FD1BF),
        on_primary: hex(0x000000),
        accent: hex(0x254B47),
        url: hex(0x6BAEE6),
        rating: hex(0xF5B041),
    };

    pub fn of(mode: Mode) -> Self {
        match mode {
            Mode::Light => Self::LIGHT,
            Mode::Dark => Self::DARK,
        }
    }

    /// The theme with another brand color (a host's `ui.brand`).
    pub fn with_brand(mut self, primary: Rgba, on_primary: Rgba) -> Self {
        self.primary = primary;
        self.on_primary = on_primary;
        self
    }
}

/// Font sizes and weights.
pub const SMALL: f32 = 12.0;
pub const BODY: f32 = 13.0;
/// Row numbers.
pub const TINY: f32 = 10.0;
pub const REGULAR: u16 = 400;
pub const MEDIUM: u16 = 500;
pub const SEMIBOLD: u16 = 600;
