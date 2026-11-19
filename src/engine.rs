//! `MenuEngine` is the single entry point the UI uses to turn a
//! [`MenuDocument`] into a PDF (and a preview bitmap). It owns the font
//! catalog, the shaping cache and the logo cache, and is `Send + Sync` so it
//! can run on a background thread.

use std::collections::HashMap;
use std::path::Path;
use std::sync::{Arc, Mutex, RwLock};
use std::time::SystemTime;

use krilla::image::Image;
use tracing::{debug, warn};

use crate::domain::{LogoSource, MenuDocument};
use crate::fonts::FontCatalog;
use crate::layout::{LayoutInput, PageLayout, RoleFonts, TextShaper, layout_menu};
use crate::render::{PdfContext, RgbaImage, rasterize_first_page, render_pdf};

pub const BUILTIN_LOGO: &[u8] = include_bytes!("../assets/logo-casapeix.png");

#[derive(Debug, Clone)]
pub struct RenderedMenu {
    pub pdf: Vec<u8>,
    /// Scale chosen by the fitting algorithm (1.0 = sizes as configured).
    pub scale: f32,
    /// `false` when the content overflows the page even at minimum scale.
    pub fits: bool,
    pub page_size_pt: (f32, f32),
}

/// Why a logo could not be used. `Display` is English (for logs); the UI
/// shows its own translated message per variant.
#[derive(Debug, Clone, PartialEq)]
pub enum LogoError {
    Unreadable(String),
    UnsupportedFormat,
    Invalid(String),
}

impl std::fmt::Display for LogoError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            LogoError::Unreadable(detail) => write!(f, "could not read the image: {detail}"),
            LogoError::UnsupportedFormat => {
                write!(f, "unsupported image format (use PNG, JPEG, WebP or GIF)")
            }
            LogoError::Invalid(detail) => write!(f, "invalid image: {detail}"),
        }
    }
}

impl std::error::Error for LogoError {}

type LogoKey = (LogoSource, Option<SystemTime>);

pub struct MenuEngine {
    fonts: Arc<FontCatalog>,
    shaper: TextShaper,
    logos: Mutex<HashMap<LogoKey, Result<Image, LogoError>>>,
    /// What `LogoSource::Default` means in a menu: the app logo from the
    /// settings (where `Default` is the bundled Casa Peix logo).
    app_logo: RwLock<LogoSource>,
}

impl MenuEngine {
    pub fn new(fonts: Arc<FontCatalog>) -> Self {
        Self {
            fonts,
            shaper: TextShaper::default(),
            logos: Mutex::new(HashMap::new()),
            app_logo: RwLock::new(LogoSource::Default),
        }
    }

    /// Sets the app logo used by menus whose logo is `LogoSource::Default`.
    pub fn set_app_logo(&self, source: LogoSource) {
        debug!(source = ?source, "app logo for menus updated");
        if let Ok(mut logo) = self.app_logo.write() {
            *logo = source;
        }
    }

    /// Resolves a menu logo choice to a concrete source.
    fn effective_source(&self, source: &LogoSource) -> LogoSource {
        match source {
            LogoSource::Default => self.app_logo.read().map(|l| l.clone()).unwrap_or_default(),
            other => other.clone(),
        }
    }

    pub fn fonts(&self) -> &FontCatalog {
        &self.fonts
    }

    /// Loads (and caches) the logo of a menu. `Default` follows the app logo.
    pub fn logo(&self, source: &LogoSource) -> Result<Option<Image>, LogoError> {
        let source = &self.effective_source(source);
        let mtime = match source {
            LogoSource::File(path) => std::fs::metadata(path).and_then(|m| m.modified()).ok(),
            _ => None,
        };
        let key = (source.clone(), mtime);
        if let Some(cached) = self.logos.lock().ok().and_then(|c| c.get(&key).cloned()) {
            return cached.map(Some);
        }
        let loaded = match source {
            LogoSource::None => return Ok(None),
            LogoSource::Default => decode_image(BUILTIN_LOGO.to_vec()),
            LogoSource::File(path) => load_image_file(path),
        };
        match &loaded {
            Ok(image) => {
                let (w, h) = image.size();
                debug!(source = ?source, width = w, height = h, "logo loaded");
            }
            Err(error) => warn!(source = ?source, %error, "logo could not be loaded"),
        }
        if let Ok(mut cache) = self.logos.lock() {
            cache.insert(key, loaded.clone());
        }
        loaded.map(Some)
    }

    pub fn layout(
        &self,
        doc: &MenuDocument,
        logo: Option<&Image>,
    ) -> anyhow::Result<(PageLayout, RoleFonts)> {
        let fonts = RoleFonts::resolve(&self.fonts, &doc.style.typography)?;
        let logo_aspect = logo.map(|img| {
            let (w, h) = img.size();
            w as f32 / h.max(1) as f32
        });
        let layout = layout_menu(&LayoutInput {
            doc,
            fonts: &fonts,
            shaper: &self.shaper,
            logo_aspect,
        });
        Ok((layout, fonts))
    }

    /// Lays out and writes the PDF. A broken custom logo is skipped rather
    /// than failing the whole export.
    pub fn render(&self, doc: &MenuDocument) -> anyhow::Result<RenderedMenu> {
        let started = std::time::Instant::now();
        let logo = self.logo(&doc.header.logo).unwrap_or(None);
        let (layout, fonts) = self.layout(doc, logo.as_ref())?;
        let title = if doc.header.title.trim().is_empty() {
            "Carta"
        } else {
            doc.header.title.trim()
        };
        let pdf = render_pdf(
            &layout,
            &PdfContext {
                fonts: &fonts,
                shaper: &self.shaper,
                logo: logo.as_ref(),
                title,
            },
        )?;
        debug!(
            scale = layout.scale,
            fits = layout.fits,
            ops = layout.ops.len(),
            pdf_bytes = pdf.len(),
            elapsed_ms = started.elapsed().as_millis() as u64,
            "menu rendered to PDF"
        );
        if !layout.fits {
            warn!(
                scale = layout.scale,
                "menu content does not fit on one page even at minimum scale"
            );
        }
        Ok(RenderedMenu {
            pdf,
            scale: layout.scale,
            fits: layout.fits,
            page_size_pt: (layout.width, layout.height),
        })
    }

    /// Renders the PDF and rasterises it for display.
    pub fn render_preview(
        &self,
        doc: &MenuDocument,
        pixels_per_point: f32,
    ) -> anyhow::Result<(RenderedMenu, RgbaImage)> {
        let rendered = self.render(doc)?;
        let started = std::time::Instant::now();
        let image = rasterize_first_page(rendered.pdf.clone(), pixels_per_point)?;
        debug!(
            width = image.width,
            height = image.height,
            elapsed_ms = started.elapsed().as_millis() as u64,
            "preview rasterised"
        );
        Ok((rendered, image))
    }
}

fn load_image_file(path: &Path) -> Result<Image, LogoError> {
    let bytes = std::fs::read(path)
        .map_err(|e| LogoError::Unreadable(format!("{}: {e}", path.display())))?;
    decode_image(bytes)
}

/// Detects the image format from its magic bytes.
fn decode_image(bytes: Vec<u8>) -> Result<Image, LogoError> {
    let data = krilla::Data::from(bytes.clone());
    let result = if bytes.starts_with(b"\x89PNG") {
        Image::from_png(data, true)
    } else if bytes.starts_with(&[0xFF, 0xD8]) {
        Image::from_jpeg(data, true)
    } else if bytes.starts_with(b"GIF8") {
        Image::from_gif(data, true)
    } else if bytes.len() > 12 && &bytes[0..4] == b"RIFF" && &bytes[8..12] == b"WEBP" {
        Image::from_webp(data, true)
    } else {
        return Err(LogoError::UnsupportedFormat);
    };
    result.map_err(|e| LogoError::Invalid(e.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn renders_sample_pdf_and_preview() {
        let catalog = FontCatalog::load_system();
        if catalog.families().is_empty() {
            return;
        }
        let engine = MenuEngine::new(Arc::new(catalog));
        let doc = MenuDocument::sample();
        let (rendered, image) = engine.render_preview(&doc, 0.5).unwrap();
        assert!(rendered.pdf.starts_with(b"%PDF"));
        assert!(rendered.fits);
        assert_eq!(image.pixels.len() as u32, image.width * image.height * 4);
        // Some pixels must be non-white (text was drawn).
        assert!(image.pixels.chunks(4).any(|p| p[0] < 200));
    }

    #[test]
    fn default_menu_logo_follows_app_logo() {
        let engine = MenuEngine::new(Arc::new(
            FontCatalog::from_database(fontdb::Database::new()),
        ));
        assert!(engine.logo(&LogoSource::Default).unwrap().is_some());
        engine.set_app_logo(LogoSource::None);
        assert!(engine.logo(&LogoSource::Default).unwrap().is_none());
        engine.set_app_logo(LogoSource::File("/definitely/missing.png".into()));
        assert!(matches!(
            engine.logo(&LogoSource::Default),
            Err(LogoError::Unreadable(_))
        ));
    }

    #[test]
    fn builtin_logo_decodes() {
        let img = decode_image(BUILTIN_LOGO.to_vec()).unwrap();
        let (w, h) = img.size();
        assert!(w > h && h > 0);
    }
}
