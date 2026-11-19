//! Layout engine: turns a [`MenuDocument`] into positioned drawing ops that
//! fit on a single page.
//!
//! The fitting strategy is a binary search over a global scale factor that
//! multiplies every font size and gap. For each candidate scale the blocks
//! are rebuilt (line breaks depend on the scale) and flowed into the
//! configured number of columns.

pub mod blocks;
pub mod flow;
pub mod model;
pub mod text;

use std::ops::Range;

use crate::domain::{MM_TO_PT, MenuDocument, Rgb, TextRole};
use blocks::{Block, BlockBuilder};
use flow::Flowed;
pub use model::{DrawOp, PageLayout};
pub use text::{RoleFonts, ShapedText, TextShaper};

pub struct LayoutInput<'a> {
    pub doc: &'a MenuDocument,
    pub fonts: &'a RoleFonts,
    pub shaper: &'a TextShaper,
    /// Width / height of the logo, `None` when there is no logo.
    pub logo_aspect: Option<f32>,
}

/// Everything computed for one candidate scale.
struct Arrangement {
    header: Block,
    header_gap: f32,
    footer: Block,
    body_height: f32,
    body_width: f32,
    column_width: f32,
    blocks: Vec<Block>,
    flowed: Flowed,
}

struct Frame {
    x: f32,
    y: f32,
    width: f32,
    height: f32,
}

fn content_frame(doc: &MenuDocument) -> (f32, f32, Frame) {
    let page = &doc.style.page;
    let (pw, ph) = page.size_pt();
    let m = &page.margins;
    let (l, r, t, b) = (
        m.left_mm.max(0.0) * MM_TO_PT,
        m.right_mm.max(0.0) * MM_TO_PT,
        m.top_mm.max(0.0) * MM_TO_PT,
        m.bottom_mm.max(0.0) * MM_TO_PT,
    );
    let frame = Frame {
        x: l,
        y: t,
        width: (pw - l - r).max(20.0),
        height: (ph - t - b).max(20.0),
    };
    (pw, ph, frame)
}

fn arrange(input: &LayoutInput, frame: &Frame, scale: f32, force: bool) -> Option<Arrangement> {
    let doc = input.doc;
    let layout = &doc.style.layout;
    let builder = BlockBuilder::new(doc, input.fonts, input.shaper, input.logo_aspect, scale);
    let spacing = doc.style.typography.spacing.clamp(0.3, 3.0);

    let header = builder.header(frame.width);
    let header_gap = if header.height > 0.0 {
        builder.size(TextRole::Section) * 0.9 * spacing
    } else {
        0.0
    };
    let footer = builder.footer(frame.width);
    let body_height =
        frame.height - header.height - header_gap - footer.height - footer.space_before;
    if body_height <= 0.0 && !force {
        return None;
    }

    let columns = layout.column_count();
    let gap = layout.gap_mm.max(0.0) * MM_TO_PT;
    let body_width = frame.width * (layout.body_width_pct.clamp(40.0, 100.0) / 100.0);
    let column_width = ((body_width - gap * (columns as f32 - 1.0)) / columns as f32).max(20.0);

    let mut blocks = Vec::new();
    let mut sections: Vec<Range<usize>> = Vec::new();
    for section in &doc.sections {
        let start = blocks.len();
        blocks.push(builder.section_header(section, column_width));
        for (i, product) in section.products.iter().enumerate() {
            blocks.push(builder.product(product, column_width, i == 0));
        }
        sections.push(start..blocks.len());
    }

    let body_height = body_height.max(0.0);
    let flowed = if force {
        flow::flow(
            &blocks,
            &sections,
            columns,
            body_height,
            layout.keep_sections_together,
            true,
        )?
    } else {
        flow::flow_balanced(
            &blocks,
            &sections,
            columns,
            body_height,
            layout.keep_sections_together,
        )?
    };

    Some(Arrangement {
        header,
        header_gap,
        footer,
        body_height,
        body_width,
        column_width,
        blocks,
        flowed,
    })
}

/// Lays out the whole menu on one page, choosing the best scale.
pub fn layout_menu(input: &LayoutInput) -> PageLayout {
    let doc = input.doc;
    let (page_w, page_h, frame) = content_frame(doc);
    let fit = &doc.style.fit;
    let min_scale = fit.min_scale.clamp(0.1, 1.0);
    let max_scale = if fit.grow_to_fill {
        fit.max_scale.clamp(1.0, 4.0)
    } else {
        1.0
    };

    let (scale, arrangement, fits) = if let Some(a) = arrange(input, &frame, max_scale, false) {
        (max_scale, a, true)
    } else if arrange(input, &frame, min_scale, false).is_none() {
        let forced =
            arrange(input, &frame, min_scale, true).expect("forced arrangement always succeeds");
        (min_scale, forced, false)
    } else {
        let (mut lo, mut hi) = (min_scale, max_scale);
        for _ in 0..20 {
            let mid = (lo + hi) / 2.0;
            if arrange(input, &frame, mid, false).is_some() {
                lo = mid;
            } else {
                hi = mid;
            }
            if hi - lo < 0.002 {
                break;
            }
        }
        (
            lo,
            arrange(input, &frame, lo, false).expect("lower bound fits"),
            true,
        )
    };

    tracing::trace!(
        scale,
        fits,
        columns = doc.style.layout.column_count(),
        blocks = arrangement.blocks.len(),
        used_height = arrangement.flowed.used_height,
        body_height = arrangement.body_height,
        "layout computed"
    );
    let ops = compose(doc, &frame, &arrangement);
    PageLayout {
        width: page_w,
        height: page_h,
        ops,
        scale,
        fits,
    }
}

fn compose(doc: &MenuDocument, frame: &Frame, a: &Arrangement) -> Vec<DrawOp> {
    let layout = &doc.style.layout;
    let mut ops = Vec::new();

    ops.extend(
        a.header
            .ops
            .iter()
            .cloned()
            .map(|op| op.translated(frame.x, frame.y)),
    );

    let body_top = frame.y + a.header.height + a.header_gap;
    let body_left = frame.x + (frame.width - a.body_width) / 2.0;
    let used = a.flowed.used_height;
    let offset = if layout.vertical_center {
        ((a.body_height - used) / 2.0).max(0.0)
    } else {
        0.0
    };
    let gap = layout.gap_mm.max(0.0) * MM_TO_PT;

    for (block, placement) in a.blocks.iter().zip(&a.flowed.placements) {
        let x = body_left + placement.column as f32 * (a.column_width + gap);
        let y = body_top + offset + placement.y;
        ops.extend(block.ops.iter().cloned().map(|op| op.translated(x, y)));
    }

    if layout.column_rule && a.flowed.columns_used > 1 {
        for column in 1..a.flowed.columns_used {
            let x = body_left + column as f32 * (a.column_width + gap) - gap / 2.0;
            ops.push(DrawOp::Line {
                x1: x,
                y1: body_top + offset,
                x2: x,
                y2: body_top + offset + used,
                width: 0.5,
                color: blend(doc.style.palette.muted, Rgb(255, 255, 255), 0.5),
                dotted: false,
            });
        }
    }

    let footer_top = frame.y + frame.height - a.footer.height;
    ops.extend(
        a.footer
            .ops
            .iter()
            .cloned()
            .map(|op| op.translated(frame.x, footer_top)),
    );
    ops
}

fn blend(a: Rgb, b: Rgb, t: f32) -> Rgb {
    let mix = |x: u8, y: u8| (x as f32 * (1.0 - t) + y as f32 * t).round() as u8;
    Rgb(mix(a.0, b.0), mix(a.1, b.1), mix(a.2, b.2))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fonts::FontCatalog;

    fn layout_for(doc: &MenuDocument) -> Option<PageLayout> {
        let catalog = FontCatalog::load_system();
        if catalog.families().is_empty() {
            return None;
        }
        let fonts = RoleFonts::resolve(&catalog, &doc.style.typography).unwrap();
        let shaper = TextShaper::default();
        Some(layout_menu(&LayoutInput {
            doc,
            fonts: &fonts,
            shaper: &shaper,
            logo_aspect: Some(2.4),
        }))
    }

    fn bottom_of_body(layout: &PageLayout) -> f32 {
        layout
            .ops
            .iter()
            .filter_map(|op| match op {
                DrawOp::Text { y, .. } => Some(*y),
                _ => None,
            })
            .fold(0.0, f32::max)
    }

    #[test]
    fn sample_fits_on_a4_in_every_column_mode() {
        for columns in 1..=3 {
            let mut doc = MenuDocument::sample();
            doc.style.layout.columns = columns;
            let Some(layout) = layout_for(&doc) else {
                return;
            };
            assert!(layout.fits, "{columns} columns should fit");
            assert!(bottom_of_body(&layout) < layout.height);
        }
    }

    #[test]
    fn huge_menu_is_scaled_down_to_fit() {
        let mut doc = MenuDocument::sample();
        let extra = doc.sections.clone();
        doc.sections.extend(extra.clone());
        let Some(layout) = layout_for(&doc) else {
            return;
        };
        assert!(layout.fits);
        assert!(layout.scale < 1.0, "scale was {}", layout.scale);
        let (_, _, frame) = content_frame(&doc);
        assert!(bottom_of_body(&layout) <= frame.y + frame.height + 0.5);

        // Four times the content only fits by switching to three columns.
        for _ in 0..2 {
            doc.sections.extend(extra.clone());
        }
        doc.style.layout.columns = 3;
        let layout = layout_for(&doc).unwrap();
        assert!(layout.fits, "3 columns, scale {}", layout.scale);
    }

    #[test]
    fn impossible_content_is_reported_as_overflow() {
        let mut doc = MenuDocument::sample();
        let extra = doc.sections.clone();
        for _ in 0..12 {
            doc.sections.extend(extra.clone());
        }
        let Some(layout) = layout_for(&doc) else {
            return;
        };
        assert!(!layout.fits);
        assert_eq!(layout.scale, doc.style.fit.min_scale);
    }

    #[test]
    fn grow_to_fill_enlarges_short_menus() {
        let mut doc = MenuDocument::sample();
        doc.sections.truncate(1);
        doc.style.fit.grow_to_fill = true;
        let Some(layout) = layout_for(&doc) else {
            return;
        };
        assert!(layout.scale > 1.0);
    }
}
