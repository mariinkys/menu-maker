//! Core of the menu maker, independent of the UI:
//!
//! ```text
//! domain   ── pure data model (menu, sections, dishes, allergens, style)
//! fonts    ── system font discovery and loading
//! layout   ── text shaping + single-page layout with auto-fit
//! render   ── PDF writer (krilla) and PDF rasteriser for the preview (hayro)
//! engine   ── facade: document → PDF / preview bitmap
//! storage  ── SQLite presets and autosave
//! settings ── app preferences: theme, brand colour, title, top-bar logo
//! paths    ── application data folder (dev.mariinkys.MenuMaker)
//! updater  ── self-update from GitHub Releases (Windows only)
//! ```
//!
//! The library only emits `tracing` events; the binary decides where they go.

pub mod domain;
pub mod engine;
pub mod fonts;
pub mod layout;
pub mod paths;
pub mod render;
pub mod settings;
pub mod storage;

/// Self-update from GitHub Releases
#[cfg(any(windows, test))]
pub mod updater;
