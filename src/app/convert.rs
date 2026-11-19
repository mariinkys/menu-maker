//! Mapping between the domain model and the Slint UI types.
//! Nothing here holds state; it only translates.

use std::rc::Rc;

use slint::{ComponentHandle, ModelRc, SharedString, VecModel};

use menu_core::domain::price::format_price;
use menu_core::domain::{
    Allergen, AllergenDisplay, FontSpec, HAlign, Language, LogoSource, MenuDocument, MenuStyle,
    Orientation, PaperSize, Rgb, Typography,
};
use menu_core::fonts::FontCatalog;
use menu_core::storage::{PresetKind, PresetSummary};

use super::editor::EditorState;
use crate::{
    DesignState, FontRow, MainWindow, MenuState, PresetRow, ProductRow, SectionRow, Strings,
};

/// Font rows of the "Typography" card, in display order (names for logs;
/// the UI labels are translated in design-panel.slint).
pub const FONT_ROWS: [&str; 6] = [
    "title",
    "subtitle",
    "section",
    "dish",
    "description",
    "price",
];

pub fn font_spec_mut(t: &mut Typography, row: usize) -> Option<&mut FontSpec> {
    Some(match row {
        0 => &mut t.title,
        1 => &mut t.subtitle,
        2 => &mut t.section,
        3 => &mut t.product,
        4 => &mut t.description,
        5 => &mut t.price,
        _ => return None,
    })
}

fn font_spec(t: &Typography, row: usize) -> &FontSpec {
    match row {
        0 => &t.title,
        1 => &t.subtitle,
        2 => &t.section,
        3 => &t.product,
        4 => &t.description,
        _ => &t.price,
    }
}

/// Replaces font families that are not installed with the system defaults
/// (serif for headings, sans for body), so the UI and PDF agree.
pub fn normalize_fonts(style: &mut MenuStyle, catalog: &FontCatalog) {
    for row in 0..FONT_ROWS.len() {
        let Some(spec) = font_spec_mut(&mut style.typography, row) else {
            continue;
        };
        let installed = catalog
            .families()
            .iter()
            .any(|f| f.eq_ignore_ascii_case(spec.family.trim()));
        if !installed {
            spec.family = if row <= 2 {
                catalog.default_serif().to_string()
            } else {
                catalog.default_sans().to_string()
            };
        }
    }
}

fn shared_strings<I: IntoIterator<Item = S>, S: Into<SharedString>>(
    items: I,
) -> ModelRc<SharedString> {
    ModelRc::new(VecModel::from(
        items.into_iter().map(Into::into).collect::<Vec<_>>(),
    ))
}

pub fn string_model(items: &[String]) -> ModelRc<SharedString> {
    shared_strings(items.iter().map(String::as_str))
}

pub fn to_slint_color(c: Rgb) -> slint::Color {
    slint::Color::from_rgb_u8(c.0, c.1, c.2)
}

// ---------------------------------------------------------------------------
// Content
// ---------------------------------------------------------------------------

pub fn sections_model(state: &EditorState) -> ModelRc<SectionRow> {
    let doc = &state.doc;
    let rows: Vec<SectionRow> = doc
        .sections
        .iter()
        .enumerate()
        .map(|(si, section)| {
            let products: Vec<ProductRow> = section
                .products
                .iter()
                .enumerate()
                .map(|(pi, p)| ProductRow {
                    name: p.name.as_str().into(),
                    description: p.description.as_str().into(),
                    price: p.price.as_str().into(),
                    price_label: format_price(&p.price, &doc.style.prices)
                        .unwrap_or_default()
                        .into(),
                    allergens: ModelRc::new(VecModel::from(
                        Allergen::ALL
                            .iter()
                            .map(|a| p.allergens.contains(a))
                            .collect::<Vec<_>>(),
                    )),
                    allergen_numbers: ModelRc::new(VecModel::from(
                        p.allergens
                            .iter()
                            .map(|a| a.number() as i32)
                            .collect::<Vec<_>>(),
                    )),
                    expanded: state.product_open(si, pi),
                })
                .collect();
            SectionRow {
                name: section.name.as_str().into(),
                note: section.note.as_str().into(),
                products: ModelRc::new(VecModel::from(products)),
                expanded: state.section_open(si),
            }
        })
        .collect();
    ModelRc::new(VecModel::from(rows))
}

pub fn logo_mode(source: &LogoSource) -> i32 {
    match source {
        LogoSource::Default => 0,
        LogoSource::File(_) => 1,
        LogoSource::None => 2,
    }
}

pub fn logo_file_name(source: &LogoSource) -> String {
    match source {
        LogoSource::File(path) => path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default(),
        _ => String::new(),
    }
}

/// Pushes header texts, logo choice and sections into the UI.
pub fn apply_content(ui: &MainWindow, state: &EditorState) {
    let menu = ui.global::<MenuState>();
    let doc = &state.doc;
    menu.set_title(doc.header.title.as_str().into());
    menu.set_subtitle(doc.header.subtitle.as_str().into());
    menu.set_footer(doc.footer.as_str().into());
    menu.set_logo_mode(logo_mode(&doc.header.logo));
    menu.set_logo_file_name(logo_file_name(&doc.header.logo).into());
    menu.set_sections(sections_model(state));
}

/// Reads the header texts (title, subtitle, footer) from the UI.
pub fn read_header(ui: &MainWindow, doc: &mut MenuDocument) {
    let menu = ui.global::<MenuState>();
    doc.header.title = menu.get_title().to_string();
    doc.header.subtitle = menu.get_subtitle().to_string();
    doc.footer = menu.get_footer().to_string();
    match menu.get_logo_mode() {
        0 => doc.header.logo = LogoSource::Default,
        2 => doc.header.logo = LogoSource::None,
        _ => {} // File: only changed through the file picker.
    }
}

// ---------------------------------------------------------------------------
// Design
// ---------------------------------------------------------------------------

fn family_index(catalog: &FontCatalog, family: &str) -> i32 {
    let families = catalog.families();
    families
        .iter()
        .position(|f| f.eq_ignore_ascii_case(family.trim()))
        .or_else(|| families.iter().position(|f| f == catalog.default_sans()))
        .unwrap_or(0) as i32
}

pub fn font_rows(style: &MenuStyle, catalog: &FontCatalog) -> ModelRc<FontRow> {
    let rows: Vec<FontRow> = FONT_ROWS
        .iter()
        .enumerate()
        .map(|(i, _)| {
            let spec = font_spec(&style.typography, i);
            FontRow {
                family_index: family_index(catalog, &spec.family),
                size: spec.size_pt.round() as i32,
                bold: spec.bold,
                italic: spec.italic,
            }
        })
        .collect();
    ModelRc::new(VecModel::from(rows))
}

pub fn apply_design(ui: &MainWindow, style: &MenuStyle, catalog: &FontCatalog) {
    let d = ui.global::<DesignState>();
    let page = &style.page;
    d.set_paper_index(page.paper.ui_index() as i32);
    let (w, h) = page.paper.dimensions_mm();
    d.set_custom_width(w.round() as i32);
    d.set_custom_height(h.round() as i32);
    d.set_orientation_index(if page.orientation == Orientation::Landscape {
        1
    } else {
        0
    });
    d.set_margin_top(page.margins.top_mm.round() as i32);
    d.set_margin_right(page.margins.right_mm.round() as i32);
    d.set_margin_bottom(page.margins.bottom_mm.round() as i32);
    d.set_margin_left(page.margins.left_mm.round() as i32);

    d.set_logo_align(style.header.logo_align.index() as i32);
    d.set_logo_height(style.header.logo_height_mm.round() as i32);
    d.set_title_align(style.header.title_align.index() as i32);
    d.set_header_divider(style.header.divider);

    let layout = &style.layout;
    d.set_columns_index(layout.column_count() as i32 - 1);
    d.set_align_index(layout.align.index() as i32);
    d.set_column_gap(layout.gap_mm.round() as i32);
    d.set_body_width(layout.body_width_pct.round() as i32);
    d.set_column_rule(layout.column_rule);
    d.set_keep_together(layout.keep_sections_together);
    d.set_vertical_center(layout.vertical_center);

    // A fresh model so the font rows are recreated with fresh bindings.
    d.set_fonts(font_rows(style, catalog));
    d.set_uppercase_sections(style.typography.uppercase_sections);
    d.set_section_dividers(style.section_dividers);
    d.set_spacing(style.typography.spacing);
    d.set_grow_to_fill(style.fit.grow_to_fill);

    d.set_accent_hex(style.palette.accent.to_hex().into());
    d.set_text_hex(style.palette.text.to_hex().into());
    d.set_muted_hex(style.palette.muted.to_hex().into());
    apply_color_swatches(ui, style);

    d.set_currency(style.prices.currency_symbol.as_str().into());
    d.set_symbol_before(style.prices.symbol_before);
    d.set_always_decimals(style.prices.always_show_decimals);
    d.set_dot_leaders(style.prices.dot_leaders);
    d.set_allergen_mode(
        AllergenDisplay::ALL
            .iter()
            .position(|a| *a == style.allergens)
            .unwrap_or(0) as i32,
    );
    d.set_show_legend(style.show_allergen_legend);
    d.set_legend_language(style.legend_language.index() as i32);
}

pub fn apply_color_swatches(ui: &MainWindow, style: &MenuStyle) {
    let d = ui.global::<DesignState>();
    d.set_accent_color(to_slint_color(style.palette.accent));
    d.set_text_color(to_slint_color(style.palette.text));
    d.set_muted_color(to_slint_color(style.palette.muted));
}

/// Reads every design control back into `style`. Font rows are handled
/// separately by `Actions.font-changed`.
pub fn read_design(ui: &MainWindow, style: &mut MenuStyle) {
    let d = ui.global::<DesignState>();
    let page = &mut style.page;
    let paper_index = d.get_paper_index().max(0) as usize;
    page.paper = PaperSize::STANDARD
        .get(paper_index)
        .copied()
        .unwrap_or(PaperSize::Custom {
            width_mm: d.get_custom_width().max(50) as f32,
            height_mm: d.get_custom_height().max(50) as f32,
        });
    page.orientation = if d.get_orientation_index() == 1 {
        Orientation::Landscape
    } else {
        Orientation::Portrait
    };
    page.margins.top_mm = d.get_margin_top().max(0) as f32;
    page.margins.right_mm = d.get_margin_right().max(0) as f32;
    page.margins.bottom_mm = d.get_margin_bottom().max(0) as f32;
    page.margins.left_mm = d.get_margin_left().max(0) as f32;

    style.header.logo_align = HAlign::from_index(d.get_logo_align().max(0) as usize);
    style.header.logo_height_mm = d.get_logo_height().max(1) as f32;
    style.header.title_align = HAlign::from_index(d.get_title_align().max(0) as usize);
    style.header.divider = d.get_header_divider();

    let layout = &mut style.layout;
    layout.columns = (d.get_columns_index().clamp(0, 2) + 1) as u8;
    layout.align = HAlign::from_index(d.get_align_index().max(0) as usize);
    layout.gap_mm = d.get_column_gap().max(0) as f32;
    layout.body_width_pct = d.get_body_width().clamp(40, 100) as f32;
    layout.column_rule = d.get_column_rule();
    layout.keep_sections_together = d.get_keep_together();
    layout.vertical_center = d.get_vertical_center();

    style.typography.uppercase_sections = d.get_uppercase_sections();
    style.section_dividers = d.get_section_dividers();
    style.typography.spacing = d.get_spacing().clamp(0.3, 3.0);
    style.fit.grow_to_fill = d.get_grow_to_fill();

    // Invalid hex text keeps the previous colour.
    if let Some(c) = Rgb::from_hex(&d.get_accent_hex()) {
        style.palette.accent = c;
    }
    if let Some(c) = Rgb::from_hex(&d.get_text_hex()) {
        style.palette.text = c;
    }
    if let Some(c) = Rgb::from_hex(&d.get_muted_hex()) {
        style.palette.muted = c;
    }

    style.prices.currency_symbol = d.get_currency().to_string();
    style.prices.symbol_before = d.get_symbol_before();
    style.prices.always_show_decimals = d.get_always_decimals();
    style.prices.dot_leaders = d.get_dot_leaders();
    style.allergens = AllergenDisplay::ALL
        .get(d.get_allergen_mode().max(0) as usize)
        .copied()
        .unwrap_or_default();
    style.show_allergen_legend = d.get_show_legend();
    style.legend_language = Language::from_index(d.get_legend_language().max(0) as usize);
}

// ---------------------------------------------------------------------------
// Presets
// ---------------------------------------------------------------------------

pub fn preset_rows(presets: &[PresetSummary]) -> ModelRc<PresetRow> {
    let rows: Vec<PresetRow> = presets
        .iter()
        .map(|p| PresetRow {
            id: p.id as i32,
            name: p.name.as_str().into(),
            is_style: p.kind == PresetKind::Style,
            updated: p.updated_at.as_str().into(),
        })
        .collect();
    ModelRc::from(Rc::new(VecModel::from(rows)))
}

/// Short English page description for logs, e.g. "A4 210x297mm portrait".
pub fn page_summary(style: &MenuStyle) -> String {
    let (w, h) = oriented_mm(style);
    let orientation = if style.page.orientation == Orientation::Landscape {
        "landscape"
    } else {
        "portrait"
    };
    format!("{:?} {w:.0}x{h:.0}mm {orientation}", style.page.paper)
}

/// Translated page description for the preview header,
/// e.g. "A4 · 210 × 297 mm · vertical".
pub fn page_label(ui: &MainWindow, style: &MenuStyle, language: Language) -> SharedString {
    let (w, h) = oriented_mm(style);
    ui.global::<Strings>().invoke_page_label(
        style.page.paper.ui_index() as i32,
        fmt_mm(w, language).into(),
        fmt_mm(h, language).into(),
        style.page.orientation == Orientation::Landscape,
    )
}

fn oriented_mm(style: &MenuStyle) -> (f32, f32) {
    let (w, h) = style.page.paper.dimensions_mm();
    if style.page.orientation == Orientation::Landscape {
        (h, w)
    } else {
        (w, h)
    }
}

/// Whole millimetres as integers, otherwise one decimal with the
/// language's decimal separator.
fn fmt_mm(v: f32, language: Language) -> String {
    if (v - v.round()).abs() < 0.05 {
        return format!("{}", v.round() as i32);
    }
    let text = format!("{v:.1}");
    if language == Language::Es {
        text.replace('.', ",")
    } else {
        text
    }
}
