//! Builds the individual pieces of a menu (header, section titles, dishes,
//! footer) at a given scale. Each piece is a [`Block`] whose drawing ops are
//! relative to its own top-left corner, so the flow step can move it freely.

use std::collections::BTreeSet;

use crate::domain::price::format_price;
use crate::domain::{
    Allergen, AllergenDisplay, HAlign, MM_TO_PT, MenuDocument, Product, Rgb, Section, TextRole,
};
use crate::fonts::LoadedFont;

use super::model::DrawOp;
use super::text::{RoleFonts, TextShaper};

#[derive(Debug, Clone, Default)]
pub struct Block {
    pub ops: Vec<DrawOp>,
    pub height: f32,
    /// Gap above the block; dropped when the block starts a column.
    pub space_before: f32,
}

/// A run of inline content placed on one line.
#[derive(Debug, Clone)]
enum Segment {
    Text {
        text: String,
        role: TextRole,
        size: f32,
        tracking: f32,
        color: Rgb,
        width: f32,
    },
    Badges {
        numbers: Vec<u8>,
        diameter: f32,
        gap: f32,
        center_dy: f32,
    },
    Space(f32),
}

impl Segment {
    fn width(&self) -> f32 {
        match self {
            Segment::Text { width, .. } => *width,
            Segment::Badges {
                numbers,
                diameter,
                gap,
                ..
            } => {
                let n = numbers.len() as f32;
                if n == 0.0 {
                    0.0
                } else {
                    n * diameter + (n - 1.0) * gap
                }
            }
            Segment::Space(w) => *w,
        }
    }
}

#[derive(Debug, Clone, Default)]
struct Line {
    segments: Vec<Segment>,
}

impl Line {
    fn width(&self) -> f32 {
        self.segments.iter().map(Segment::width).sum()
    }
}

pub struct BlockBuilder<'a> {
    doc: &'a MenuDocument,
    fonts: &'a RoleFonts,
    shaper: &'a TextShaper,
    logo_aspect: Option<f32>,
    pub scale: f32,
}

const UPPERCASE_TRACKING: f32 = 0.08;

impl<'a> BlockBuilder<'a> {
    pub fn new(
        doc: &'a MenuDocument,
        fonts: &'a RoleFonts,
        shaper: &'a TextShaper,
        logo_aspect: Option<f32>,
        scale: f32,
    ) -> Self {
        Self {
            doc,
            fonts,
            shaper,
            logo_aspect,
            scale,
        }
    }

    pub fn size(&self, role: TextRole) -> f32 {
        (self.doc.style.typography.spec(role).size_pt * self.scale).max(2.0)
    }

    fn spacing(&self) -> f32 {
        self.doc.style.typography.spacing.clamp(0.3, 3.0)
    }

    fn font(&self, role: TextRole) -> &LoadedFont {
        self.fonts.get(role)
    }

    fn text_segment(
        &self,
        text: &str,
        role: TextRole,
        size: f32,
        tracking: f32,
        color: Rgb,
    ) -> Segment {
        let width = self.shaper.width(self.font(role), text, size, tracking);
        Segment::Text {
            text: text.to_string(),
            role,
            size,
            tracking,
            color,
            width,
        }
    }

    fn wrap(
        &self,
        text: &str,
        role: TextRole,
        size: f32,
        tracking: f32,
        max_width: f32,
    ) -> Vec<String> {
        self.shaper
            .wrap(self.font(role), text, size, tracking, max_width.max(1.0))
    }

    /// Baseline offset from the top of a line box of height `line_height`.
    fn baseline_in_line(&self, role: TextRole, size: f32, line_height: f32) -> f32 {
        let f = self.font(role);
        let content = (f.ascender - f.descender) * size;
        (line_height - content) / 2.0 + f.ascender * size
    }

    /// Emits a line aligned inside `[x, x + width]` at `baseline`.
    fn emit_line(
        &self,
        line: &Line,
        x: f32,
        width: f32,
        align: HAlign,
        baseline: f32,
        ops: &mut Vec<DrawOp>,
    ) {
        let line_width = line.width();
        let mut cursor = match align {
            HAlign::Left => x,
            HAlign::Center => x + (width - line_width) / 2.0,
            HAlign::Right => x + width - line_width,
        };
        for segment in &line.segments {
            match segment {
                Segment::Text {
                    text,
                    role,
                    size,
                    tracking,
                    color,
                    width,
                } => {
                    ops.push(DrawOp::Text {
                        x: cursor,
                        y: baseline,
                        text: text.clone(),
                        role: *role,
                        size: *size,
                        tracking: *tracking,
                        color: *color,
                    });
                    cursor += width;
                }
                Segment::Badges {
                    numbers,
                    diameter,
                    gap,
                    center_dy,
                } => {
                    for number in numbers {
                        self.emit_badge(
                            *number,
                            cursor + diameter / 2.0,
                            baseline + center_dy,
                            *diameter,
                            ops,
                        );
                        cursor += diameter + gap;
                    }
                    cursor -= gap;
                }
                Segment::Space(w) => cursor += w,
            }
        }
    }

    fn emit_badge(&self, number: u8, cx: f32, cy: f32, diameter: f32, ops: &mut Vec<DrawOp>) {
        let palette = &self.doc.style.palette;
        ops.push(DrawOp::Circle {
            cx,
            cy,
            r: diameter / 2.0,
            color: palette.accent,
        });
        let label = number.to_string();
        let size = diameter * if number >= 10 { 0.52 } else { 0.62 };
        let font = self.font(TextRole::Badge);
        let w = self.shaper.width(font, &label, size, 0.0);
        ops.push(DrawOp::Text {
            x: cx - w / 2.0,
            y: cy + font.cap_height * size / 2.0,
            text: label,
            role: TextRole::Badge,
            size,
            tracking: 0.0,
            color: Rgb(255, 255, 255),
        });
    }

    /// Inline allergen marker for a dish, according to the display mode.
    fn allergen_segment(
        &self,
        allergens: &BTreeSet<Allergen>,
        line_role: TextRole,
        line_size: f32,
    ) -> Option<Segment> {
        if allergens.is_empty() {
            return None;
        }
        let numbers: Vec<u8> = allergens.iter().map(|a| a.number()).collect();
        match self.doc.style.allergens {
            AllergenDisplay::Hidden => None,
            AllergenDisplay::Badges => {
                let diameter = self.size(TextRole::Description) * 1.2;
                let cap = self.font(line_role).cap_height * line_size;
                Some(Segment::Badges {
                    numbers,
                    diameter,
                    gap: diameter * 0.2,
                    center_dy: -cap / 2.0,
                })
            }
            AllergenDisplay::Numbers => {
                let text = format!(
                    "({})",
                    numbers
                        .iter()
                        .map(|n| n.to_string())
                        .collect::<Vec<_>>()
                        .join(", ")
                );
                let size = self.size(TextRole::Description) * 0.9;
                Some(self.text_segment(
                    &text,
                    TextRole::Badge,
                    size,
                    0.0,
                    self.doc.style.palette.muted,
                ))
            }
        }
    }

    fn short_rule(
        &self,
        x: f32,
        available: f32,
        length: f32,
        align: HAlign,
        y: f32,
        width: f32,
    ) -> DrawOp {
        let length = length.min(available);
        let start = match align {
            HAlign::Left => x,
            HAlign::Center => x + (available - length) / 2.0,
            HAlign::Right => x + available - length,
        };
        DrawOp::Line {
            x1: start,
            y1: y,
            x2: start + length,
            y2: y,
            width,
            color: self.doc.style.palette.accent,
            dotted: false,
        }
    }

    // -----------------------------------------------------------------------
    // Header
    // -----------------------------------------------------------------------

    pub fn header(&self, width: f32) -> Block {
        let style = &self.doc.style;
        let header = &self.doc.header;
        let mut ops = Vec::new();
        let mut y = 0.0;

        if let Some(aspect) = self.logo_aspect {
            // The logo shrinks with the menu but never grows past its set size.
            let mut h = style.header.logo_height_mm.max(1.0) * MM_TO_PT * self.scale.min(1.0);
            let mut w = h * aspect;
            if w > width {
                w = width;
                h = w / aspect;
            }
            let x = match style.header.logo_align {
                HAlign::Left => 0.0,
                HAlign::Center => (width - w) / 2.0,
                HAlign::Right => width - w,
            };
            ops.push(DrawOp::Logo {
                x,
                y,
                width: w,
                height: h,
            });
            y += h;
        }

        let align = style.header.title_align;
        let title = header.title.trim();
        if !title.is_empty() {
            let size = self.size(TextRole::Title);
            if y > 0.0 {
                y += size * 0.35;
            }
            let lh = size * 1.18;
            for text in self.wrap(title, TextRole::Title, size, 0.0, width) {
                let line = Line {
                    segments: vec![self.text_segment(
                        &text,
                        TextRole::Title,
                        size,
                        0.0,
                        style.palette.accent,
                    )],
                };
                self.emit_line(
                    &line,
                    0.0,
                    width,
                    align,
                    y + self.baseline_in_line(TextRole::Title, size, lh),
                    &mut ops,
                );
                y += lh;
            }
        }

        let subtitle = header.subtitle.trim();
        if !subtitle.is_empty() {
            let size = self.size(TextRole::Subtitle);
            if y > 0.0 {
                y += size * 0.15;
            }
            let lh = size * 1.3;
            for text in self.wrap(subtitle, TextRole::Subtitle, size, 0.03, width) {
                let line = Line {
                    segments: vec![self.text_segment(
                        &text,
                        TextRole::Subtitle,
                        size,
                        0.03,
                        style.palette.muted,
                    )],
                };
                self.emit_line(
                    &line,
                    0.0,
                    width,
                    align,
                    y + self.baseline_in_line(TextRole::Subtitle, size, lh),
                    &mut ops,
                );
                y += lh;
            }
        }

        if style.header.divider && y > 0.0 {
            y += self.size(TextRole::Subtitle) * 0.6;
            let stroke = 0.9 * self.scale.min(1.2);
            ops.push(self.short_rule(0.0, width, 150.0 * self.scale, align, y, stroke));
            y += stroke;
        }

        Block {
            ops,
            height: y,
            space_before: 0.0,
        }
    }

    // -----------------------------------------------------------------------
    // Sections and dishes
    // -----------------------------------------------------------------------

    pub fn section_header(&self, section: &Section, width: f32) -> Block {
        let style = &self.doc.style;
        let align = style.layout.align;
        let size = self.size(TextRole::Section);
        let uppercase = style.typography.uppercase_sections;
        let tracking = if uppercase { UPPERCASE_TRACKING } else { 0.0 };
        let name = if uppercase {
            section.name.trim().to_uppercase()
        } else {
            section.name.trim().to_string()
        };

        let mut ops = Vec::new();
        let mut y = 0.0;
        let lh = size * 1.2;
        for text in self.wrap(&name, TextRole::Section, size, tracking, width) {
            let line = Line {
                segments: vec![self.text_segment(
                    &text,
                    TextRole::Section,
                    size,
                    tracking,
                    style.palette.accent,
                )],
            };
            self.emit_line(
                &line,
                0.0,
                width,
                align,
                y + self.baseline_in_line(TextRole::Section, size, lh),
                &mut ops,
            );
            y += lh;
        }

        let note = section.note.trim();
        if !note.is_empty() {
            let note_size = self.size(TextRole::SectionNote);
            let nlh = note_size * 1.3;
            for text in self.wrap(note, TextRole::SectionNote, note_size, 0.0, width) {
                let line = Line {
                    segments: vec![self.text_segment(
                        &text,
                        TextRole::SectionNote,
                        note_size,
                        0.0,
                        style.palette.muted,
                    )],
                };
                self.emit_line(
                    &line,
                    0.0,
                    width,
                    align,
                    y + self.baseline_in_line(TextRole::SectionNote, note_size, nlh),
                    &mut ops,
                );
                y += nlh;
            }
        }

        if style.section_dividers {
            y += size * 0.25;
            let stroke = 0.6 * self.scale.min(1.2);
            ops.push(self.short_rule(0.0, width, 36.0 * self.scale, align, y, stroke));
            y += stroke;
        }

        Block {
            ops,
            height: y,
            space_before: size * 1.15 * self.spacing(),
        }
    }

    pub fn product(&self, product: &Product, width: f32, first_in_section: bool) -> Block {
        let style = &self.doc.style;
        let palette = &style.palette;
        let align = style.layout.align;
        let name_size = self.size(TextRole::Product);
        let desc_size = self.size(TextRole::Description);
        let price_size = self.size(TextRole::Price);
        let name_lh = name_size * 1.22;

        let price = format_price(&product.price, &style.prices);
        let badges = self.allergen_segment(&product.allergens, TextRole::Product, name_size);
        let badge_gap = name_size * 0.4;
        let name = product.name.trim();

        let mut ops = Vec::new();
        let mut y = 0.0;

        // With left alignment the price sits in its own column on the right.
        let price_column = align == HAlign::Left && price.is_some();
        let text_width;

        if price_column {
            let price_text = price.unwrap_or_default();
            let price_seg = self.text_segment(
                &price_text,
                TextRole::Price,
                price_size,
                0.0,
                palette.accent,
            );
            let price_w = price_seg.width();
            let gutter = name_size * 0.9;
            text_width = (width - price_w - gutter).max(width * 0.4);

            let mut lines: Vec<Line> = self
                .wrap(name, TextRole::Product, name_size, 0.0, text_width)
                .into_iter()
                .map(|t| Line {
                    segments: vec![self.text_segment(
                        &t,
                        TextRole::Product,
                        name_size,
                        0.0,
                        palette.text,
                    )],
                })
                .collect();
            if lines.is_empty() {
                lines.push(Line::default());
            }
            attach_inline(&mut lines, badges, badge_gap, text_width);

            for (i, line) in lines.iter().enumerate() {
                let baseline = y + self.baseline_in_line(TextRole::Product, name_size, name_lh);
                self.emit_line(line, 0.0, text_width, HAlign::Left, baseline, &mut ops);
                if i == 0 {
                    let price_x = width - price_w;
                    self.emit_line(
                        &Line {
                            segments: vec![price_seg.clone()],
                        },
                        price_x,
                        price_w,
                        HAlign::Left,
                        baseline,
                        &mut ops,
                    );
                    if style.prices.dot_leaders {
                        let start = line.width() + name_size * 0.5;
                        let end = price_x - name_size * 0.5;
                        if end - start > name_size {
                            ops.push(DrawOp::Line {
                                x1: start,
                                y1: baseline - name_size * 0.05,
                                x2: end,
                                y2: baseline - name_size * 0.05,
                                width: (name_size * 0.09).max(0.4),
                                color: palette.muted,
                                dotted: true,
                            });
                        }
                    }
                }
                y += name_lh;
            }
        } else {
            text_width = width;
            let mut lines: Vec<Line> = self
                .wrap(name, TextRole::Product, name_size, 0.0, width)
                .into_iter()
                .map(|t| Line {
                    segments: vec![self.text_segment(
                        &t,
                        TextRole::Product,
                        name_size,
                        0.0,
                        palette.text,
                    )],
                })
                .collect();
            if lines.is_empty() {
                lines.push(Line::default());
            }
            let price_seg = price
                .map(|p| self.text_segment(&p, TextRole::Price, price_size, 0.0, palette.accent));
            attach_inline(&mut lines, price_seg, name_size * 0.7, width);
            attach_inline(&mut lines, badges, badge_gap, width);
            for line in &lines {
                let baseline = y + self.baseline_in_line(TextRole::Product, name_size, name_lh);
                self.emit_line(line, 0.0, width, align, baseline, &mut ops);
                y += name_lh;
            }
        }

        let description = product.description.trim();
        if !description.is_empty() {
            let desc_lh = desc_size * 1.3;
            y += desc_size * 0.05;
            let desc_align = if price_column { HAlign::Left } else { align };
            for text in self.wrap(
                description,
                TextRole::Description,
                desc_size,
                0.0,
                text_width,
            ) {
                let line = Line {
                    segments: vec![self.text_segment(
                        &text,
                        TextRole::Description,
                        desc_size,
                        0.0,
                        palette.muted,
                    )],
                };
                let baseline = y + self.baseline_in_line(TextRole::Description, desc_size, desc_lh);
                self.emit_line(&line, 0.0, text_width, desc_align, baseline, &mut ops);
                y += desc_lh;
            }
        }

        let factor = if first_in_section { 0.6 } else { 0.85 };
        Block {
            ops,
            height: y,
            space_before: name_size * factor * self.spacing(),
        }
    }

    // -----------------------------------------------------------------------
    // Footer: allergen legend + free text
    // -----------------------------------------------------------------------

    pub fn footer(&self, width: f32) -> Block {
        let style = &self.doc.style;
        let palette = &style.palette;
        let size = self.size(TextRole::Legend);
        let mut ops = Vec::new();
        let mut y = 0.0;

        let used = self.doc.used_allergens();
        if style.show_allergen_legend
            && style.allergens != AllergenDisplay::Hidden
            && !used.is_empty()
        {
            let item_gap = size * 1.6;
            let mut items: Vec<Line> = Vec::new();
            for allergen in &used {
                let marker = match style.allergens {
                    AllergenDisplay::Badges => {
                        let diameter = self.size(TextRole::Description) * 1.2;
                        let cap = self.font(TextRole::Legend).cap_height * size;
                        Segment::Badges {
                            numbers: vec![allergen.number()],
                            diameter,
                            gap: 0.0,
                            center_dy: -cap / 2.0,
                        }
                    }
                    _ => self.text_segment(
                        &allergen.number().to_string(),
                        TextRole::Badge,
                        size,
                        0.0,
                        palette.accent,
                    ),
                };
                items.push(Line {
                    segments: vec![
                        marker,
                        Segment::Space(size * 0.35),
                        self.text_segment(
                            allergen.label(style.legend_language),
                            TextRole::Legend,
                            size,
                            0.0,
                            palette.muted,
                        ),
                    ],
                });
            }
            // Pack items into centred rows of similar length: find the
            // narrowest width that still needs no more rows than `width`.
            let row_count = pack_rows(&items, item_gap, width).len();
            let (mut lo, mut hi) = (0.0_f32, width);
            for _ in 0..20 {
                let mid = (lo + hi) / 2.0;
                let rows = pack_rows(&items, item_gap, mid);
                if rows.len() <= row_count && rows.iter().all(|r| r.width() <= mid + 0.01) {
                    hi = mid;
                } else {
                    lo = mid;
                }
            }
            let rows = pack_rows(&items, item_gap, hi);
            let lh = (self.size(TextRole::Description) * 1.2).max(size) * 1.35;
            for row in &rows {
                let baseline = y + self.baseline_in_line(TextRole::Legend, size, lh);
                self.emit_line(row, 0.0, width, HAlign::Center, baseline, &mut ops);
                y += lh;
            }
        }

        let footer_text = self.doc.footer.trim();
        if !footer_text.is_empty() {
            let fsize = self.size(TextRole::Footer);
            if y > 0.0 {
                y += fsize * 0.3;
            }
            let lh = fsize * 1.35;
            for text in self.wrap(footer_text, TextRole::Footer, fsize, 0.0, width) {
                let line = Line {
                    segments: vec![self.text_segment(
                        &text,
                        TextRole::Footer,
                        fsize,
                        0.0,
                        palette.muted,
                    )],
                };
                let baseline = y + self.baseline_in_line(TextRole::Footer, fsize, lh);
                self.emit_line(&line, 0.0, width, HAlign::Center, baseline, &mut ops);
                y += lh;
            }
        }

        let space_before = if y > 0.0 {
            self.size(TextRole::Section) * 0.9 * self.spacing()
        } else {
            0.0
        };
        Block {
            ops,
            height: y,
            space_before,
        }
    }
}

/// Greedily packs legend items into rows no wider than `width`.
fn pack_rows(items: &[Line], gap: f32, width: f32) -> Vec<Line> {
    let mut rows: Vec<Line> = Vec::new();
    let mut current = Line::default();
    for item in items {
        if !current.segments.is_empty() && current.width() + gap + item.width() > width {
            rows.push(std::mem::take(&mut current));
        }
        if !current.segments.is_empty() {
            current.segments.push(Segment::Space(gap));
        }
        current.segments.extend(item.segments.iter().cloned());
    }
    if !current.segments.is_empty() {
        rows.push(current);
    }
    rows
}

/// Appends `segment` to the last line when it fits, otherwise on a new line.
fn attach_inline(lines: &mut Vec<Line>, segment: Option<Segment>, gap: f32, max_width: f32) {
    let Some(segment) = segment else { return };
    let last = lines.last_mut().expect("at least one line");
    if last.segments.is_empty() {
        last.segments.push(segment);
    } else if last.width() + gap + segment.width() <= max_width {
        last.segments.push(Segment::Space(gap));
        last.segments.push(segment);
    } else {
        lines.push(Line {
            segments: vec![segment],
        });
    }
}
