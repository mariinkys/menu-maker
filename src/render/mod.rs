//! Output backends for a computed layout.

pub mod pdf;
pub mod raster;

pub use pdf::{PdfContext, render_pdf};
pub use raster::{RgbaImage, rasterize_first_page};
