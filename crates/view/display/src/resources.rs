//! The resource table a list's ids refer to: Tessera icons by id, emoji by atlas id (a glyph of
//! the emoji face the engine rasterizes), and attachment pictures by key (decoded by the player).

use std::collections::HashMap;
use std::sync::Arc;

use kition_table_fonts::FaceId;
use vello_cpu::Pixmap;

/// Icons, emoji and images, with a version a list's header names.
#[derive(Clone, Debug, Default)]
pub struct Resources {
    /// Bumped whenever an id is added.
    pub version: u16,
    icons: Vec<&'static kition_tessera_icons::Icon>,
    names: Vec<&'static str>,
    emoji: Vec<(FaceId, u32)>,
    images: HashMap<u32, Arc<Pixmap>>,
}

impl Resources {
    /// The id of Tessera's icon `name` (added on first use), or `None` for an unknown icon.
    pub fn icon(&mut self, name: &str) -> Option<u16> {
        if let Some(index) = self.names.iter().position(|known| *known == name) {
            return u16::try_from(index).ok();
        }
        let icon = kition_tessera_icons::icon(name)?;
        let id = u16::try_from(self.icons.len()).ok()?;
        self.icons.push(icon);
        self.names.push(icon.name);
        self.version = self.version.wrapping_add(1);
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

    /// The atlas id of glyph `glyph` of the emoji face `face` (added on first use).
    pub fn emoji(&mut self, face: FaceId, glyph: u32) -> u32 {
        if let Some(index) = self.emoji.iter().position(|known| *known == (face, glyph)) {
            return u32::try_from(index).unwrap_or(u32::MAX);
        }
        self.emoji.push((face, glyph));
        self.version = self.version.wrapping_add(1);
        u32::try_from(self.emoji.len() - 1).unwrap_or(u32::MAX)
    }

    /// The face and glyph of an atlas id.
    pub fn emoji_of(&self, atlas: u32) -> Option<(FaceId, u32)> {
        self.emoji.get(atlas as usize).copied()
    }

    /// Hands the player the decoded picture of image `key`.
    pub fn set_image(&mut self, key: u32, pixmap: Pixmap) {
        self.images.insert(key, Arc::new(pixmap));
        self.version = self.version.wrapping_add(1);
    }

    pub fn image(&self, key: u32) -> Option<&Arc<Pixmap>> {
        self.images.get(&key)
    }
}
