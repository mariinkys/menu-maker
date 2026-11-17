//! Visual configuration of a menu: page, layout, typography, colours.
//!
//! Every struct uses `#[serde(default)]` so presets saved by older versions
//! keep loading when new options are added.

use serde::{Deserialize, Serialize};

use super::Language;

pub const MM_TO_PT: f32 = 72.0 / 25.4;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HAlign {
    Left,
    #[default]
    Center,
    Right,
}

impl HAlign {
    pub const ALL: [HAlign; 3] = [HAlign::Left, HAlign::Center, HAlign::Right];

    pub fn index(self) -> usize {
        Self::ALL.iter().position(|a| *a == self).unwrap_or(1)
    }
    pub fn from_index(i: usize) -> Self {
        Self::ALL.get(i).copied().unwrap_or_default()
    }
}

// ---------------------------------------------------------------------------
// Page
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PaperSize {
    #[default]
    A4,
    A5,
    A3,
    Letter,
    Legal,
    Custom { width_mm: f32, height_mm: f32 },
}

impl PaperSize {
    /// Standard sizes shown in the UI, in order. `Custom` is always last.
    pub const STANDARD: [PaperSize; 5] = [
        PaperSize::A4,
        PaperSize::A5,
        PaperSize::A3,
        PaperSize::Letter,
        PaperSize::Legal,
    ];

    /// Portrait dimensions in millimetres.
    pub fn dimensions_mm(self) -> (f32, f32) {
        match self {
            PaperSize::A4 => (210.0, 297.0),
            PaperSize::A5 => (148.0, 210.0),
            PaperSize::A3 => (297.0, 420.0),
            PaperSize::Letter => (215.9, 279.4),
            PaperSize::Legal => (215.9, 355.6),
            PaperSize::Custom { width_mm, height_mm } => {
                (width_mm.clamp(50.0, 2000.0), height_mm.clamp(50.0, 2000.0))
            }
        }
    }

    pub fn ui_index(self) -> usize {
        Self::STANDARD
            .iter()
            .position(|p| *p == self)
            .unwrap_or(Self::STANDARD.len())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Orientation {
    #[default]
    Portrait,
    Landscape,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Margins {
    pub top_mm: f32,
    pub right_mm: f32,
    pub bottom_mm: f32,
    pub left_mm: f32,
}

impl Default for Margins {
    fn default() -> Self {
        Self { top_mm: 12.0, right_mm: 14.0, bottom_mm: 12.0, left_mm: 14.0 }
    }
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct PageSetup {
    pub paper: PaperSize,
    pub orientation: Orientation,
    pub margins: Margins,
}

impl PageSetup {
    /// Page size in PDF points, orientation applied.
    pub fn size_pt(&self) -> (f32, f32) {
        let (w, h) = self.paper.dimensions_mm();
        let (w, h) = match self.orientation {
            Orientation::Portrait => (w, h),
            Orientation::Landscape => (h, w),
        };
        (w * MM_TO_PT, h * MM_TO_PT)
    }
}

// ---------------------------------------------------------------------------
// Layout
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ColumnLayout {
    /// 1, 2 or 3.
    pub columns: u8,
    /// Alignment of section titles and dishes inside each column.
    pub align: HAlign,
    pub gap_mm: f32,
    /// Thin vertical rule between columns.
    pub column_rule: bool,
    /// Avoid splitting a section across two columns when possible.
    pub keep_sections_together: bool,
    /// Vertically centre the dishes in the free space.
    pub vertical_center: bool,
    /// Width of the dishes area as a percentage of the printable width
    /// (40–100). Narrower bodies read better in single-column menus.
    pub body_width_pct: f32,
}

impl Default for ColumnLayout {
    fn default() -> Self {
        Self {
            columns: 1,
            align: HAlign::Center,
            gap_mm: 10.0,
            column_rule: false,
            keep_sections_together: true,
            vertical_center: true,
            body_width_pct: 85.0,
        }
    }
}

impl ColumnLayout {
    pub fn column_count(&self) -> usize {
        self.columns.clamp(1, 3) as usize
    }
}

// ---------------------------------------------------------------------------
// Typography
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct FontSpec {
    pub family: String,
    pub size_pt: f32,
    pub bold: bool,
    pub italic: bool,
}

impl Default for FontSpec {
    fn default() -> Self {
        Self { family: String::new(), size_pt: 10.0, bold: false, italic: false }
    }
}

impl FontSpec {
    fn new(family: &str, size_pt: f32, bold: bool, italic: bool) -> Self {
        Self { family: family.into(), size_pt, bold, italic }
    }
}

/// Text roles that can be styled independently.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TextRole {
    Title,
    Subtitle,
    Section,
    SectionNote,
    Product,
    Description,
    Price,
    Badge,
    Legend,
    Footer,
}

impl TextRole {
    pub const ALL: [TextRole; 10] = [
        TextRole::Title,
        TextRole::Subtitle,
        TextRole::Section,
        TextRole::SectionNote,
        TextRole::Product,
        TextRole::Description,
        TextRole::Price,
        TextRole::Badge,
        TextRole::Legend,
        TextRole::Footer,
    ];
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Typography {
    pub title: FontSpec,
    pub subtitle: FontSpec,
    pub section: FontSpec,
    pub product: FontSpec,
    pub description: FontSpec,
    pub price: FontSpec,
    /// Write section titles in capitals.
    pub uppercase_sections: bool,
    /// Multiplier for every vertical gap (0.5 – 2.0).
    pub spacing: f32,
}

impl Default for Typography {
    fn default() -> Self {
        // Families are resolved against the system later; unknown names fall
        // back to a sensible default, see `fonts::FontCatalog`.
        Self {
            title: FontSpec::new("Lora", 30.0, false, true),
            subtitle: FontSpec::new("Lora", 12.0, false, false),
            section: FontSpec::new("Lora", 15.0, true, false),
            product: FontSpec::new("Inter", 11.0, true, false),
            description: FontSpec::new("Inter", 9.0, false, true),
            price: FontSpec::new("Inter", 11.0, true, false),
            uppercase_sections: true,
            spacing: 1.0,
        }
    }
}

impl Typography {
    /// Font used for a role. Derived roles reuse the closest configured font.
    pub fn spec(&self, role: TextRole) -> FontSpec {
        match role {
            TextRole::Title => self.title.clone(),
            TextRole::Subtitle => self.subtitle.clone(),
            TextRole::Section => self.section.clone(),
            TextRole::SectionNote => FontSpec {
                size_pt: self.description.size_pt,
                bold: false,
                italic: true,
                ..self.section.clone()
            },
            TextRole::Product => self.product.clone(),
            TextRole::Description => self.description.clone(),
            TextRole::Price => self.price.clone(),
            TextRole::Badge => FontSpec {
                size_pt: self.description.size_pt,
                bold: true,
                italic: false,
                ..self.description.clone()
            },
            TextRole::Legend | TextRole::Footer => FontSpec {
                size_pt: (self.description.size_pt * 0.9).max(5.0),
                bold: false,
                italic: false,
                ..self.description.clone()
            },
        }
    }
}

// ---------------------------------------------------------------------------
// Colours, prices, allergens, fitting
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Rgb(pub u8, pub u8, pub u8);

impl Rgb {
    pub fn to_hex(self) -> String {
        format!("#{:02X}{:02X}{:02X}", self.0, self.1, self.2)
    }

    pub fn from_hex(s: &str) -> Option<Rgb> {
        let s = s.trim().trim_start_matches('#');
        let expanded: String = match s.len() {
            3 => s.chars().flat_map(|c| [c, c]).collect(),
            6 => s.to_string(),
            _ => return None,
        };
        let v = u32::from_str_radix(&expanded, 16).ok()?;
        Some(Rgb((v >> 16) as u8, (v >> 8) as u8, v as u8))
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Palette {
    /// Titles, section names, badges, decorative lines.
    pub accent: Rgb,
    /// Dish names and prices.
    pub text: Rgb,
    /// Descriptions, legend and footer.
    pub muted: Rgb,
}

impl Default for Palette {
    fn default() -> Self {
        Self {
            accent: Rgb(0xAD, 0x00, 0x00), // Casa Peix red, sampled from the logo.
            text: Rgb(0x22, 0x22, 0x22),
            muted: Rgb(0x5E, 0x5E, 0x5E),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct PriceStyle {
    pub currency_symbol: String,
    pub symbol_before: bool,
    pub always_show_decimals: bool,
    /// Dotted leader between the dish name and a right-aligned price.
    pub dot_leaders: bool,
}

impl Default for PriceStyle {
    fn default() -> Self {
        Self {
            currency_symbol: "€".into(),
            symbol_before: false,
            always_show_decimals: false,
            dot_leaders: true,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AllergenDisplay {
    /// Small numbered circles.
    #[default]
    Badges,
    /// Plain numbers, e.g. "(1, 7)".
    Numbers,
    Hidden,
}

impl AllergenDisplay {
    pub const ALL: [AllergenDisplay; 3] =
        [AllergenDisplay::Badges, AllergenDisplay::Numbers, AllergenDisplay::Hidden];
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct FitOptions {
    /// Allow scaling text above 100 % so the menu fills the page.
    pub grow_to_fill: bool,
    /// Upper bound used when growing.
    pub max_scale: f32,
    /// Lower bound used when shrinking; below this the menu overflows.
    pub min_scale: f32,
}

impl Default for FitOptions {
    fn default() -> Self {
        Self { grow_to_fill: true, max_scale: 1.5, min_scale: 0.35 }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct HeaderStyle {
    pub logo_align: HAlign,
    pub logo_height_mm: f32,
    pub title_align: HAlign,
    /// Decorative rule under the header.
    pub divider: bool,
}

impl Default for HeaderStyle {
    fn default() -> Self {
        Self { logo_align: HAlign::Center, logo_height_mm: 24.0, title_align: HAlign::Center, divider: true }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct MenuStyle {
    pub page: PageSetup,
    pub header: HeaderStyle,
    pub layout: ColumnLayout,
    pub typography: Typography,
    pub palette: Palette,
    pub prices: PriceStyle,
    pub allergens: AllergenDisplay,
    pub show_allergen_legend: bool,
    /// Language of the printed allergen legend.
    pub legend_language: Language,
    /// Small accent line under each section title.
    pub section_dividers: bool,
    pub fit: FitOptions,
}

impl Default for MenuStyle {
    fn default() -> Self {
        Self {
            page: PageSetup::default(),
            header: HeaderStyle::default(),
            layout: ColumnLayout::default(),
            typography: Typography::default(),
            palette: Palette::default(),
            prices: PriceStyle::default(),
            allergens: AllergenDisplay::default(),
            show_allergen_legend: true,
            legend_language: Language::Es,
            section_dividers: true,
            fit: FitOptions::default(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hex_roundtrip() {
        assert_eq!(Rgb::from_hex("#AD0000"), Some(Rgb(0xAD, 0, 0)));
        assert_eq!(Rgb::from_hex("fff"), Some(Rgb(255, 255, 255)));
        assert_eq!(Rgb::from_hex("zz"), None);
        assert_eq!(Rgb(1, 2, 255).to_hex(), "#0102FF");
    }

    #[test]
    fn a4_landscape_in_points() {
        let page = PageSetup { orientation: Orientation::Landscape, ..Default::default() };
        let (w, h) = page.size_pt();
        assert!((w - 841.89).abs() < 0.1 && (h - 595.28).abs() < 0.1);
    }
}
