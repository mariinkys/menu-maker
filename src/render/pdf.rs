//! Writes a [`PageLayout`] to PDF with krilla.

use anyhow::{Context, anyhow};
use krilla::color::rgb;
use krilla::geom::{PathBuilder, Point, Size, Transform};
use krilla::image::Image;
use krilla::metadata::Metadata;
use krilla::num::NormalizedF32;
use krilla::page::PageSettings;
use krilla::paint::{Fill, LineCap, Stroke, StrokeDash};
use krilla::surface::Surface;
use krilla::text::{GlyphId, KrillaGlyph};
use krilla::Document;

use crate::domain::Rgb;
use crate::layout::{DrawOp, PageLayout, RoleFonts, TextShaper};

/// Kappa for approximating a quarter circle with a cubic Bézier.
const KAPPA: f32 = 0.552_284_8;

pub struct PdfContext<'a> {
    pub fonts: &'a RoleFonts,
    pub shaper: &'a TextShaper,
    pub logo: Option<&'a Image>,
    pub title: &'a str,
}

pub fn render_pdf(layout: &PageLayout, ctx: &PdfContext) -> anyhow::Result<Vec<u8>> {
    let mut document = Document::new();
    let settings = PageSettings::from_wh(layout.width, layout.height)
        .ok_or_else(|| anyhow!("invalid page size"))?;
    {
        let mut page = document.start_page_with(settings);
        let mut surface = page.surface();
        for op in &layout.ops {
            draw_op(&mut surface, op, ctx);
        }
        surface.finish();
        page.finish();
    }
    document.set_metadata(
        Metadata::new()
            .title(ctx.title.to_string())
            .creator("menu-maker".to_string())
            .language("es-ES".to_string()),
    );
    document
        .finish()
        .map_err(|e| anyhow!("{e:?}"))
        .context("could not write the PDF")
}

fn paint(color: Rgb) -> krilla::paint::Paint {
    rgb::Color::new(color.0, color.1, color.2).into()
}

fn draw_op(surface: &mut Surface, op: &DrawOp, ctx: &PdfContext) {
    match op {
        DrawOp::Text { x, y, text, role, size, tracking, color } => {
            let font = ctx.fonts.get(*role);
            let shaped = ctx.shaper.shape(font, text);
            let glyphs: Vec<KrillaGlyph> = shaped
                .glyphs
                .iter()
                .map(|g| {
                    KrillaGlyph::new(
                        GlyphId::new(g.glyph_id),
                        g.x_advance + tracking,
                        g.x_offset,
                        g.y_offset,
                        0.0,
                        g.cluster.clone(),
                        None,
                    )
                })
                .collect();
            surface.set_stroke(None);
            surface.set_fill(Some(Fill { paint: paint(*color), ..Fill::default() }));
            surface.draw_glyphs(Point::from_xy(*x, *y), &glyphs, font.krilla.clone(), text, *size, false);
        }
        DrawOp::Line { x1, y1, x2, y2, width, color, dotted } => {
            let mut builder = PathBuilder::new();
            builder.move_to(*x1, *y1);
            builder.line_to(*x2, *y2);
            let Some(path) = builder.finish() else { return };
            let dash = dotted.then(|| StrokeDash { array: vec![0.0, width * 3.0], offset: 0.0 });
            surface.set_fill(None);
            surface.set_stroke(Some(Stroke {
                paint: paint(*color),
                width: *width,
                line_cap: if *dotted { LineCap::Round } else { LineCap::Butt },
                opacity: NormalizedF32::ONE,
                dash,
                ..Stroke::default()
            }));
            surface.draw_path(&path);
            surface.set_stroke(None);
        }
        DrawOp::Circle { cx, cy, r, color } => {
            let Some(path) = circle_path(*cx, *cy, *r) else { return };
            surface.set_stroke(None);
            surface.set_fill(Some(Fill { paint: paint(*color), ..Fill::default() }));
            surface.draw_path(&path);
        }
        DrawOp::Logo { x, y, width, height } => {
            let (Some(image), Some(size)) = (ctx.logo, Size::from_wh(*width, *height)) else { return };
            surface.push_transform(&Transform::from_translate(*x, *y));
            surface.draw_image(image.clone(), size);
            surface.pop();
        }
    }
}

fn circle_path(cx: f32, cy: f32, r: f32) -> Option<krilla::geom::Path> {
    let k = r * KAPPA;
    let mut b = PathBuilder::new();
    b.move_to(cx + r, cy);
    b.cubic_to(cx + r, cy + k, cx + k, cy + r, cx, cy + r);
    b.cubic_to(cx - k, cy + r, cx - r, cy + k, cx - r, cy);
    b.cubic_to(cx - r, cy - k, cx - k, cy - r, cx, cy - r);
    b.cubic_to(cx + k, cy - r, cx + r, cy - k, cx + r, cy);
    b.close();
    b.finish()
}
