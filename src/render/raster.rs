//! Rasterises a PDF page with hayro for the on-screen preview, so the
//! preview *is* the PDF.

use anyhow::anyhow;
use hayro::hayro_interpret::InterpreterSettings;
use hayro::hayro_syntax::Pdf;
use hayro::vello_cpu::color::palette::css::WHITE;
use hayro::{PixmapSettings, RenderCache, RenderSettings};

/// Straight RGBA8 pixels (opaque: the page background is white).
#[derive(Debug, Clone)]
pub struct RgbaImage {
    pub width: u32,
    pub height: u32,
    pub pixels: Vec<u8>,
}

/// Renders the first page at `pixels_per_point` (1.0 = 72 dpi).
pub fn rasterize_first_page(pdf_bytes: Vec<u8>, pixels_per_point: f32) -> anyhow::Result<RgbaImage> {
    let pdf = Pdf::new(pdf_bytes).map_err(|e| anyhow!("invalid PDF: {e:?}"))?;
    let page = pdf.pages().iter().next().ok_or_else(|| anyhow!("the PDF has no pages"))?;
    let cache = RenderCache::new();
    let pixmap = hayro::render(
        page,
        &cache,
        &InterpreterSettings::default(),
        &RenderSettings::default(),
        &PixmapSettings { x_scale: pixels_per_point, y_scale: pixels_per_point, bg_color: WHITE },
    );
    Ok(RgbaImage {
        width: pixmap.width() as u32,
        height: pixmap.height() as u32,
        // Opaque white background, so premultiplied == straight alpha.
        pixels: pixmap.data_as_u8_slice().to_vec(),
    })
}
