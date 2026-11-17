//! Text shaping, measuring and line breaking.
//!
//! Shaping is done once here with harfrust and the very same glyph runs are
//! handed to the PDF writer, so what the layout measures is exactly what
//! ends up on paper.

use std::collections::HashMap;
use std::ops::Range;
use std::sync::{Arc, Mutex};

use crate::domain::{TextRole, Typography};
use crate::fonts::{FontCatalog, LoadedFont};

/// A glyph with metrics normalised to the em size (multiply by font size).
#[derive(Debug, Clone)]
pub struct ShapedGlyph {
    pub glyph_id: u32,
    pub x_advance: f32,
    pub x_offset: f32,
    pub y_offset: f32,
    /// Byte range of the source text this glyph represents.
    pub cluster: Range<usize>,
}

#[derive(Debug, Clone)]
pub struct ShapedText {
    pub glyphs: Vec<ShapedGlyph>,
    /// Total advance in ems, without tracking.
    pub advance: f32,
}

impl ShapedText {
    /// Width in points at `size`, with `tracking` (ems) between glyphs.
    pub fn width(&self, size: f32, tracking: f32) -> f32 {
        let gaps = self.glyphs.len().saturating_sub(1) as f32;
        (self.advance + tracking * gaps) * size
    }
}

/// Shapes text and caches the results; shared by all layout passes.
#[derive(Default)]
pub struct TextShaper {
    cache: Mutex<HashMap<(u64, String), Arc<ShapedText>>>,
}

impl TextShaper {
    const MAX_CACHE: usize = 50_000;

    pub fn shape(&self, font: &LoadedFont, text: &str) -> Arc<ShapedText> {
        let key = (font.id, text.to_string());
        if let Some(hit) = self.cache.lock().ok().and_then(|c| c.get(&key).cloned()) {
            return hit;
        }
        let shaped = Arc::new(shape_uncached(font, text));
        if let Ok(mut cache) = self.cache.lock() {
            if cache.len() > Self::MAX_CACHE {
                cache.clear();
            }
            cache.insert(key, shaped.clone());
        }
        shaped
    }

    pub fn width(&self, font: &LoadedFont, text: &str, size: f32, tracking: f32) -> f32 {
        if text.is_empty() {
            return 0.0;
        }
        self.shape(font, text).width(size, tracking)
    }

    /// Greedy word wrap. Words longer than `max_width` are split by character.
    pub fn wrap(&self, font: &LoadedFont, text: &str, size: f32, tracking: f32, max_width: f32) -> Vec<String> {
        let mut lines = Vec::new();
        for paragraph in text.split('\n') {
            let mut current = String::new();
            for word in paragraph.split_whitespace() {
                let candidate =
                    if current.is_empty() { word.to_string() } else { format!("{current} {word}") };
                if self.width(font, &candidate, size, tracking) <= max_width {
                    current = candidate;
                    continue;
                }
                if !current.is_empty() {
                    lines.push(std::mem::take(&mut current));
                }
                if self.width(font, word, size, tracking) <= max_width {
                    current = word.to_string();
                } else {
                    // Hard-break a very long word.
                    for ch in word.chars() {
                        let mut next = current.clone();
                        next.push(ch);
                        if !current.is_empty() && self.width(font, &next, size, tracking) > max_width {
                            lines.push(std::mem::take(&mut current));
                            current.push(ch);
                        } else {
                            current = next;
                        }
                    }
                }
            }
            if !current.is_empty() {
                lines.push(current);
            }
        }
        lines
    }
}

fn shape_uncached(font: &LoadedFont, text: &str) -> ShapedText {
    let empty = || ShapedText { glyphs: Vec::new(), advance: 0.0 };
    // Default scale is 1.0, so positions come back in font units.
    let shaper_font = harfrust::ShaperFont::new(&font.shaper);
    let upem = font.shaper.units_per_em().max(1) as f32;

    let mut buffer = harfrust::Buffer::new();
    buffer.push_str(text);
    buffer.guess_segment_properties();
    buffer.set_direction(harfrust::Direction::LeftToRight);
    if harfrust::shape(&shaper_font, &mut buffer, harfrust::ShapeOptions::new()).is_err() {
        return empty();
    }

    let infos = buffer.glyph_infos();
    let positions = buffer.glyph_positions();
    let mut glyphs = Vec::with_capacity(infos.len());
    let mut advance = 0.0;
    for (i, (info, pos)) in infos.iter().zip(positions).enumerate() {
        let start = info.cluster as usize;
        // The cluster ends where the next different cluster begins.
        let end = infos[i + 1..]
            .iter()
            .map(|n| n.cluster as usize)
            .find(|c| *c != start)
            .unwrap_or(text.len());
        let x_advance = pos.x_advance as f32 / upem;
        advance += x_advance;
        glyphs.push(ShapedGlyph {
            glyph_id: info.glyph_id,
            x_advance,
            x_offset: pos.x_offset as f32 / upem,
            y_offset: pos.y_offset as f32 / upem,
            cluster: start..end.max(start),
        });
    }
    ShapedText { glyphs, advance }
}

/// The concrete font for every text role of a menu.
#[derive(Clone)]
pub struct RoleFonts {
    fonts: HashMap<TextRole, LoadedFont>,
}

impl RoleFonts {
    pub fn resolve(catalog: &FontCatalog, typography: &Typography) -> anyhow::Result<Self> {
        let mut fonts = HashMap::new();
        for role in TextRole::ALL {
            let font = catalog
                .load(&typography.spec(role))
                .ok_or_else(|| anyhow::anyhow!("no usable font installed on this system"))?;
            fonts.insert(role, font);
        }
        Ok(Self { fonts })
    }

    pub fn get(&self, role: TextRole) -> &LoadedFont {
        // `resolve` fills every role, so this cannot fail.
        &self.fonts[&role]
    }
}
