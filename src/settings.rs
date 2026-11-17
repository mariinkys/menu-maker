//! Application-level preferences
use serde::{Deserialize, Serialize};

use crate::domain::{Language, LogoSource, Rgb};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ThemeMode {
    #[default]
    Light,
    Dark,
}

impl ThemeMode {
    pub const ALL: [ThemeMode; 2] = [ThemeMode::Light, ThemeMode::Dark];

    pub fn index(self) -> usize {
        Self::ALL.iter().position(|m| *m == self).unwrap_or(0)
    }

    pub fn from_index(i: usize) -> Self {
        Self::ALL.get(i).copied().unwrap_or_default()
    }
}

/// Casa Peix red
pub const DEFAULT_BRAND: Rgb = Rgb(0xAD, 0x00, 0x00);

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct AppSettings {
    /// Interface language (Slint bundled translations).
    pub language: Language,
    pub theme: ThemeMode,
    /// Top bar, buttons and accents in the editor. Also the accent used
    /// when the menu design is reset.
    pub brand_color: Rgb,
    /// Window title and text next to the logo in the top bar. Empty means
    /// the default, translated title ("Casa Peix · Editor de Cartas").
    pub app_title: String,
    /// Logo shown in the top bar.
    pub logo: LogoSource,
    /// Paint the logo in the text colour of the top bar (good for
    /// single-colour logos; turn off for full-colour ones).
    pub colorize_logo: bool,
}

impl Default for AppSettings {
    fn default() -> Self {
        Self {
            language: Language::Es,
            theme: ThemeMode::Light,
            brand_color: DEFAULT_BRAND,
            app_title: String::new(),
            logo: LogoSource::Default,
            colorize_logo: true,
        }
    }
}

fn relative_luminance(c: Rgb) -> f32 {
    fn channel(c: u8) -> f32 {
        let c = c as f32 / 255.0;
        if c <= 0.039_28 {
            c / 12.92
        } else {
            ((c + 0.055) / 1.055).powf(2.4)
        }
    }
    0.2126 * channel(c.0) + 0.7152 * channel(c.1) + 0.0722 * channel(c.2)
}

/// WCAG contrast ratio between two colours (1.0 – 21.0).
pub fn contrast_ratio(a: Rgb, b: Rgb) -> f32 {
    let (la, lb) = (relative_luminance(a), relative_luminance(b));
    (la.max(lb) + 0.05) / (la.min(lb) + 0.05)
}

/// Text colour (near-black or white) with the best contrast on `background`.
pub fn contrast_text(background: Rgb) -> Rgb {
    let white = Rgb(255, 255, 255);
    let black = Rgb(0x1A, 0x1A, 0x1A);
    if contrast_ratio(white, background) >= contrast_ratio(black, background) {
        white
    } else {
        black
    }
}

/// `color` adjusted towards black or white (whichever the background
/// contrasts with) until it reaches a 4.5:1 contrast on `background`, so
/// brand-coloured text stays readable on light and dark surfaces.
pub fn readable_on(color: Rgb, background: Rgb) -> Rgb {
    const TARGET: f32 = 4.5;
    let target = contrast_text(background);
    let mix = |t: f32| {
        let m = |a: u8, b: u8| (a as f32 + (b as f32 - a as f32) * t).round() as u8;
        Rgb(
            m(color.0, target.0),
            m(color.1, target.1),
            m(color.2, target.2),
        )
    };
    (0..=20)
        .map(|i| mix(i as f32 * 0.05))
        .find(|c| contrast_ratio(*c, background) >= TARGET)
        .unwrap_or(target)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_are_casa_peix() {
        let s = AppSettings::default();
        assert_eq!(s.brand_color, Rgb(0xAD, 0, 0));
        assert_eq!(s.logo, LogoSource::Default);
        assert_eq!(s.language, Language::Es);
        assert!(s.app_title.is_empty(), "empty = translated default title");
    }

    #[test]
    fn contrast_picks_readable_text() {
        assert_eq!(contrast_text(Rgb(0xAD, 0, 0)), Rgb(255, 255, 255));
        assert_eq!(contrast_text(Rgb(0xFF, 0xD6, 0x00)), Rgb(0x1A, 0x1A, 0x1A));
        assert_eq!(contrast_text(Rgb(0x1F, 0x3A, 0x5F)), Rgb(255, 255, 255));
    }

    #[test]
    fn brand_text_is_readable_on_both_themes() {
        let white = Rgb(255, 255, 255);
        let dark_surface = Rgb(0x24, 0x20, 0x1E);
        for brand in [
            Rgb(0xAD, 0, 0),
            Rgb(0xE0, 0xA1, 0x00),
            Rgb(0x22, 0x22, 0x22),
            Rgb(0xFF, 0xFF, 0x00),
        ] {
            assert!(contrast_ratio(readable_on(brand, white), white) >= 4.5);
            assert!(contrast_ratio(readable_on(brand, dark_surface), dark_surface) >= 4.5);
        }
        // Already readable colours are left untouched.
        assert_eq!(readable_on(Rgb(0xAD, 0, 0), white), Rgb(0xAD, 0, 0));
    }

    #[test]
    fn partial_json_keeps_defaults() {
        let s: AppSettings = serde_json::from_str(r#"{"theme":"dark"}"#).unwrap();
        assert_eq!(s.theme, ThemeMode::Dark);
        assert!(s.colorize_logo);
        assert_eq!(s.language, Language::Es);
    }
}
