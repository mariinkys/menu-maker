//! Applies [`AppSettings`] to the UI (theme, brand colour, title, top-bar
//! logo) and reads the settings dialog back.

use std::path::Path;

use slint::ComponentHandle;
use tracing::warn;

use menu_core::domain::{Language, LogoSource, Rgb};
use menu_core::settings::{AppSettings, ThemeMode, contrast_text, readable_on};

use super::convert::{logo_file_name, logo_mode, to_slint_color};
use crate::{Branding, MainWindow, SettingsState, Theme};

/// Loads an image file for the top bar with Slint's image loader.
pub fn load_logo_image(path: &Path) -> Result<slint::Image, slint::LoadImageError> {
    slint::Image::load_from_path(path)
}

/// Switches the bundled Slint translation. English is the source language.
pub fn select_language(language: Language) {
    match slint::select_bundled_translation(language.code()) {
        Ok(()) => tracing::info!(language = language.code(), "interface language selected"),
        Err(error) => {
            warn!(language = language.code(), %error, "could not select the interface language")
        }
    }
}

/// Pushes theme and branding to the UI. A custom logo that can no longer be
/// read falls back to the built-in one (and is reported in the log).
pub fn apply(ui: &MainWindow, settings: &AppSettings) {
    let dark = settings.theme == ThemeMode::Dark;
    // Must match `Theme.surface` in ui/theme.slint.
    let surface = if dark {
        Rgb(0x24, 0x20, 0x1E)
    } else {
        Rgb(0xFF, 0xFF, 0xFF)
    };
    let theme = ui.global::<Theme>();
    theme.set_dark(dark);
    theme.set_brand(to_slint_color(settings.brand_color));
    theme.set_on_brand(to_slint_color(contrast_text(settings.brand_color)));
    theme.set_brand_text(to_slint_color(readable_on(settings.brand_color, surface)));

    let branding = ui.global::<Branding>();
    branding.set_app_title(settings.app_title.trim().into());
    branding.set_colorize_logo(settings.colorize_logo);
    match &settings.logo {
        LogoSource::Default => {
            branding.set_has_logo(true);
            branding.set_use_custom_logo(false);
        }
        LogoSource::None => branding.set_has_logo(false),
        LogoSource::File(path) => match load_logo_image(path) {
            Ok(image) => {
                branding.set_custom_logo(image);
                branding.set_use_custom_logo(true);
                branding.set_has_logo(true);
            }
            Err(error) => {
                warn!(path = %path.display(), ?error, "app logo unavailable, using the built-in one");
                branding.set_use_custom_logo(false);
                branding.set_has_logo(true);
            }
        },
    }
    ui.global::<SettingsState>()
        .set_brand_preview(to_slint_color(settings.brand_color));
}

/// Fills every field of the settings dialog from `settings`.
pub fn fill_form(ui: &MainWindow, settings: &AppSettings) {
    let form = ui.global::<SettingsState>();
    form.set_language_index(settings.language.index() as i32);
    form.set_theme_index(settings.theme.index() as i32);
    form.set_brand_hex(settings.brand_color.to_hex().into());
    form.set_app_title(settings.app_title.as_str().into());
    form.set_logo_mode(logo_mode(&settings.logo));
    form.set_logo_file_name(logo_file_name(&settings.logo).into());
    form.set_colorize_logo(settings.colorize_logo);
}

/// Reads the dialog into `settings`. The custom logo path is only changed by
/// the file picker; an invalid colour keeps the previous one.
pub fn read_form(ui: &MainWindow, settings: &mut AppSettings) {
    let form = ui.global::<SettingsState>();
    settings.language = Language::from_index(form.get_language_index().max(0) as usize);
    settings.theme = ThemeMode::from_index(form.get_theme_index().max(0) as usize);
    if let Some(color) = Rgb::from_hex(&form.get_brand_hex()) {
        settings.brand_color = color;
    }
    settings.app_title = form.get_app_title().to_string();
    match form.get_logo_mode() {
        0 => settings.logo = LogoSource::Default,
        2 => settings.logo = LogoSource::None,
        _ => {}
    }
    settings.colorize_logo = form.get_colorize_logo();
}
