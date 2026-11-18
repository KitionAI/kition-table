//! The resource table a list's ids refer to: Tessera icons by id (the page gets each as a tile the
//! engine rasterizes, as an emoji), emoji by atlas id (a glyph of the emoji face the engine
//! rasterizes), and attachment pictures by key (the attachment they
//! show, their size, and their pixels where the engine decoded them).
//!
//! The page gets what it lacks as [`Entry`] records in a [`ResourceTable`] (bytes: see
//! [`crate::encode_resources`]); the headless rasterizer reads `Resources` directly.

use std::collections::BTreeMap;
use std::sync::Arc;

use kition_table_fonts::FaceId;
use vello_cpu::Pixmap;

use crate::raster::{IconStyle, Tile, emoji_tile, icon_tile};

/// One attachment picture: what it shows, its size, and its pixels where they were decoded.
#[derive(Clone, Debug, Default)]
struct Picture {
    source: String,
    /// Width and height as shown (after the picture's orientation); (0, 0) when unknown.
    size: (u32, u32),
    pixels: Option<Arc<Pixmap>>,
}

/// Icons, emoji and images, with a version a list's header names.
#[derive(Clone, Debug, Default)]
pub struct Resources {
    /// Bumped whenever an id is added or what it refers to changes.
    pub version: u16,
    icons: Vec<&'static kition_tessera_icons::Icon>,
    names: Vec<&'static str>,
    emoji: Vec<(FaceId, u32)>,
    images: BTreeMap<u32, Picture>,
}

/// One resource as the page receives it.
#[derive(Clone, Debug, PartialEq)]
pub enum Entry {
    /// A Tessera icon's tile drawn with one style at one device pixel ratio (straight RGBA).
    Icon { id: u16, style: IconStyle, dpr: f32, tile: Tile },
    /// An emoji's tile at one font size and device pixel ratio (straight RGBA).
    Emoji { atlas: u32, size: f32, dpr: f32, tile: Tile },
    /// An attachment picture: the attachment's url (`kition-blob:<sha256>` for one held in the
    /// file) the page loads and decodes, and its size as shown ((0, 0) when the engine does not
    /// know it).
    Image { key: u32, width: u32, height: u32, source: String },
}

/// Entries that bring a page's resources to `version`.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ResourceTable {
    pub version: u16,
    pub entries: Vec<Entry>,
}

impl Resources {
    fn bump(&mut self) {
        self.version = self.version.wrapping_add(1);
    }

    /// The id of Tessera's icon `name` (added on first use), or `None` for an unknown icon.
    pub fn icon(&mut self, name: &str) -> Option<u16> {
        if let Some(index) = self.names.iter().position(|known| *known == name) {
            return u16::try_from(index).ok();
        }
        let icon = kition_tessera_icons::icon(name)?;
        let id = u16::try_from(self.icons.len()).ok()?;
        self.icons.push(icon);
        self.names.push(icon.name);
        self.bump();
        Some(id)
    }

    /// The icon of an id.
    pub fn icon_of(&self, id: u16) -> Option<&'static kition_tessera_icons::Icon> {
        self.icons.get(usize::from(id)).copied()
    }

    /// The name of an icon id.
    pub fn icon_name(&self, id: u16) -> Option<&'static str> {
        self.names.get(usize::from(id)).copied()
    }

    /// How many icons there are (ids run from 0).
    pub fn icon_count(&self) -> usize {
        self.icons.len()
    }

    /// The atlas id of glyph `glyph` of the emoji face `face` (added on first use).
    pub fn emoji(&mut self, face: FaceId, glyph: u32) -> u32 {
        if let Some(index) = self.emoji.iter().position(|known| *known == (face, glyph)) {
            return u32::try_from(index).unwrap_or(u32::MAX);
        }
        self.emoji.push((face, glyph));
        self.bump();
        u32::try_from(self.emoji.len() - 1).unwrap_or(u32::MAX)
    }

    /// The face and glyph of an atlas id.
    pub fn emoji_of(&self, atlas: u32) -> Option<(FaceId, u32)> {
        self.emoji.get(atlas as usize).copied()
    }

    /// The key of the attachment picture `source` (its url), added on first use.
    pub fn image(&mut self, source: &str) -> u32 {
        if let Some((key, _)) = self.images.iter().find(|(_, picture)| picture.source == source) {
            return *key;
        }
        let key = self.images.last_key_value().map_or(0, |(key, _)| key.saturating_add(1));
        self.images.insert(key, Picture { source: source.to_owned(), ..Picture::default() });
        self.bump();
        key
    }

    /// The keys of the attachment pictures, in order.
    pub fn image_keys(&self) -> impl Iterator<Item = u32> + '_ {
        self.images.keys().copied()
    }

    /// The url of image `key`.
    pub fn image_source(&self, key: u32) -> Option<&str> {
        self.images.get(&key).map(|picture| picture.source.as_str())
    }

    /// The size image `key` is shown at (after its orientation), when known.
    pub fn image_size(&self, key: u32) -> Option<(u32, u32)> {
        self.images.get(&key).map(|picture| picture.size).filter(|(width, height)| *width > 0 && *height > 0)
    }

    /// Sets the size image `key` is shown at (read from the picture's header, without decoding).
    pub fn set_image_size(&mut self, key: u32, width: u32, height: u32) {
        let picture = self.images.entry(key).or_default();
        if picture.size != (width, height) {
            picture.size = (width, height);
            self.bump();
        }
    }

    /// Hands the rasterizer the decoded pixels of image `key` (a thumbnail: its size may be
    /// smaller than the picture's, its proportions are the same). Sets the image's size when it
    /// was not known.
    pub fn set_image(&mut self, key: u32, pixmap: Pixmap) {
        let picture = self.images.entry(key).or_default();
        if picture.size == (0, 0) {
            picture.size = (u32::from(pixmap.width()), u32::from(pixmap.height()));
        }
        picture.pixels = Some(Arc::new(pixmap));
        self.bump();
    }

    /// The decoded pixels of image `key`.
    pub fn pixels(&self, key: u32) -> Option<&Arc<Pixmap>> {
        self.images.get(&key).and_then(|picture| picture.pixels.as_ref())
    }

    /// Icon `id`'s tile drawn with `style` at `dpr` as the page receives it.
    pub fn icon_entry(&self, id: u16, style: IconStyle, dpr: f64) -> Option<Entry> {
        let tile = icon_tile(self, id, style, dpr)?;
        Some(Entry::Icon { id, style, dpr: dpr as f32, tile })
    }

    /// Emoji `atlas`'s tile at font size `size` and `dpr` as the page receives it.
    pub fn emoji_entry(&self, atlas: u32, size: f32, dpr: f64) -> Option<Entry> {
        let tile = emoji_tile(self, atlas, size, dpr)?;
        Some(Entry::Emoji { atlas, size, dpr: dpr as f32, tile })
    }

    /// Image `key` as the page receives it (its url and size; the page decodes it).
    pub fn image_entry(&self, key: u32) -> Option<Entry> {
        let picture = self.images.get(&key)?;
        Some(Entry::Image { key, width: picture.size.0, height: picture.size.1, source: picture.source.clone() })
    }
}
