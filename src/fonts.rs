//! System font discovery and loading.
//!
//! `FontCatalog` lists the families installed on the machine and loads the
//! concrete face (regular / bold / italic) for a [`FontSpec`]. Missing
//! families fall back to a sensible default so presets made on another
//! computer still render.

use std::collections::{BTreeSet, HashMap};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use fontdb::{Database, Family, Query, Stretch, Style, Weight};
use skrifa::instance::{LocationRef, Size};
use skrifa::{FontRef, MetadataProvider, Tag};
use tracing::{debug, info, warn};

use crate::domain::FontSpec;

/// A font face ready for shaping (harfrust) and embedding (krilla).
#[derive(Clone)]
pub struct LoadedFont {
    /// Process-unique id, handy as a cache key.
    pub id: u64,
    pub family: String,
    pub data: Arc<Vec<u8>>,
    pub index: u32,
    /// Ascender / descender as a fraction of the em size (descender < 0).
    pub ascender: f32,
    pub descender: f32,
    /// Height of capital letters as a fraction of the em size.
    pub cap_height: f32,
    /// Shaping handle; parsed once and cheap to clone (layout tables are cached).
    pub shaper: harfrust::Font,
    pub krilla: krilla::text::Font,
}

impl std::fmt::Debug for LoadedFont {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LoadedFont").field("id", &self.id).field("family", &self.family).finish()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct FaceKey {
    family: String,
    bold: bool,
    italic: bool,
}

pub struct FontCatalog {
    db: Database,
    families: Vec<String>,
    default_serif: String,
    default_sans: String,
    cache: Mutex<HashMap<FaceKey, LoadedFont>>,
    next_id: AtomicU64,
}

const PREFERRED_SANS: &[&str] = &[
    "Inter", "Lato", "Open Sans", "Source Sans 3", "Roboto", "Segoe UI", "Helvetica Neue",
    "Helvetica", "Arial", "Noto Sans", "DejaVu Sans", "Liberation Sans", "FreeSans",
];
const PREFERRED_SERIF: &[&str] = &[
    "Lora", "Playfair Display", "Cormorant Garamond", "EB Garamond", "Georgia", "Cambria",
    "Times New Roman", "Noto Serif", "DejaVu Serif", "Liberation Serif", "FreeSerif",
];

impl FontCatalog {
    /// Scans the fonts installed on the system.
    pub fn load_system() -> Self {
        let started = std::time::Instant::now();
        let mut db = Database::new();
        db.load_system_fonts();
        let catalog = Self::from_database(db);
        info!(
            faces = catalog.db.len(),
            families = catalog.families.len(),
            default_sans = %catalog.default_sans,
            default_serif = %catalog.default_serif,
            elapsed_ms = started.elapsed().as_millis() as u64,
            "system fonts loaded"
        );
        if catalog.families.is_empty() {
            warn!("no usable fonts found on this system; text cannot be rendered");
        }
        catalog
    }

    pub fn from_database(mut db: Database) -> Self {
        let mut families = BTreeSet::new();
        for face in db.faces() {
            let Some((name, _)) = face.families.first() else { continue };
            if name.starts_with('.') || name.is_empty() {
                continue;
            }
            families.insert(name.clone());
        }
        // Keep only families that can actually draw Latin text.
        let families: Vec<String> = families
            .into_iter()
            .filter(|family| Self::family_supports_latin(&db, family))
            .collect();

        let pick = |preferred: &[&str]| {
            preferred
                .iter()
                .find(|p| families.iter().any(|f| f.eq_ignore_ascii_case(p)))
                .map(|s| s.to_string())
                .or_else(|| families.first().cloned())
                .unwrap_or_default()
        };
        let default_sans = pick(PREFERRED_SANS);
        let default_serif = pick(PREFERRED_SERIF);
        db.set_sans_serif_family(default_sans.clone());
        db.set_serif_family(default_serif.clone());

        Self {
            db,
            families,
            default_serif,
            default_sans,
            cache: Mutex::new(HashMap::new()),
            next_id: AtomicU64::new(1),
        }
    }

    fn family_supports_latin(db: &Database, family: &str) -> bool {
        let query = Query { families: &[Family::Name(family)], ..Query::default() };
        let Some(id) = db.query(&query) else { return false };
        db.with_face_data(id, |data, index| {
            FontRef::from_index(data, index).is_ok_and(|font| {
                let has_outlines = [b"glyf", b"CFF ", b"CFF2"]
                    .iter()
                    .any(|tag| font.table_data(Tag::new(tag)).is_some());
                let charmap = font.charmap();
                has_outlines && ['a', 'Z', 'é', 'ñ', '1'].iter().all(|c| charmap.map(*c).is_some())
            })
        })
        .unwrap_or(false)
    }

    /// Sorted list of usable family names.
    pub fn families(&self) -> &[String] {
        &self.families
    }

    pub fn default_sans(&self) -> &str {
        &self.default_sans
    }

    pub fn default_serif(&self) -> &str {
        &self.default_serif
    }

    /// Returns the installed family name matching `name` (case-insensitive),
    /// or the default sans family.
    pub fn resolve_family_name(&self, name: &str) -> String {
        self.families
            .iter()
            .find(|f| f.eq_ignore_ascii_case(name.trim()))
            .cloned()
            .unwrap_or_else(|| self.default_sans.clone())
    }

    /// Loads the face that best matches the spec, falling back gracefully.
    pub fn load(&self, spec: &FontSpec) -> Option<LoadedFont> {
        let family = self.resolve_family_name(&spec.family);
        let key = FaceKey { family: family.clone(), bold: spec.bold, italic: spec.italic };
        if let Some(font) = self.cache.lock().ok()?.get(&key) {
            return Some(font.clone());
        }
        if !family.eq_ignore_ascii_case(spec.family.trim()) {
            warn!(requested = %spec.family, fallback = %family, "font family not installed, using fallback");
        }

        let query = Query {
            families: &[Family::Name(&family), Family::SansSerif],
            weight: if spec.bold { Weight::BOLD } else { Weight::NORMAL },
            stretch: Stretch::Normal,
            style: if spec.italic { Style::Italic } else { Style::Normal },
        };
        let Some(id) = self.db.query(&query) else {
            warn!(%family, bold = spec.bold, italic = spec.italic, "no matching font face");
            return None;
        };
        let (data, index) = self.db.with_face_data(id, |data, index| (data.to_vec(), index))?;
        let data = Arc::new(data);

        let (ascender, descender, cap_height) = {
            let font = FontRef::from_index(&data, index).ok()?;
            // Unscaled metrics are in font units; normalise to the em size.
            let metrics = font.metrics(Size::unscaled(), LocationRef::default());
            let upem = metrics.units_per_em.max(1) as f32;
            let cap = metrics.cap_height.map_or(0.7, |c| c / upem);
            (metrics.ascent / upem, metrics.descent / upem, cap)
        };
        let blob: Arc<dyn AsRef<[u8]> + Send + Sync> = data.clone();
        let shaper = harfrust::Font::new(harfrust::font::Blob::from(blob), index)?;
        let krilla = krilla::text::Font::new(krilla::Data::from(data.clone()), index)?;
        let font = LoadedFont {
            id: self.next_id.fetch_add(1, Ordering::Relaxed),
            family,
            data,
            index,
            ascender,
            descender,
            cap_height,
            shaper,
            krilla,
        };
        debug!(family = %font.family, bold = spec.bold, italic = spec.italic, bytes = font.data.len(), "font face loaded");
        self.cache.lock().ok()?.insert(key, font.clone());
        Some(font)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn loads_some_system_font_with_fallback() {
        let catalog = FontCatalog::load_system();
        if catalog.families().is_empty() {
            return; // Headless CI without fonts.
        }
        let spec = FontSpec { family: "Definitely Not Installed".into(), ..Default::default() };
        let font = catalog.load(&spec).expect("fallback font");
        assert_eq!(font.family, catalog.default_sans());
        assert!(font.ascender > 0.0 && font.descender < 0.0);
    }
}
