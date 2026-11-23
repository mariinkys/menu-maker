//! UI controller: owns the editor state and connects Slint callbacks to the
//! core (engine, storage). The `.slint` files only describe the
//! interface; every decision is taken here or in `menu_core`.

mod about;
mod branding;
mod convert;
mod editor;
mod preview;

/// Auto-update dialog and flow (Windows only).
#[cfg(windows)]
mod update;

use std::cell::RefCell;
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::Arc;
use std::time::{Duration, Instant};

use slint::{ComponentHandle, SharedString, Timer, TimerMode};

use menu_core::domain::price::format_price;
use menu_core::domain::{LogoSource, MenuDocument, MenuStyle};
use menu_core::engine::{LogoError, MenuEngine};
use menu_core::fonts::FontCatalog;
use menu_core::settings::AppSettings;
use menu_core::storage::{PresetPayload, PresetStore};
use tracing::{debug, error, info, trace, warn};

use crate::{
    Actions, AppState, Catalog, MainWindow, MenuState, PresetState, PreviewState, SettingsState,
    Strings,
};
use editor::EditorState;
use preview::PreviewWorker;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum StatusKind {
    Info = 0,
    Success = 1,
    Error = 2,
}

/// Destructive actions that need a second click to confirm.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Confirm {
    NewDocument,
    DeleteSection(usize),
    DeletePreset(i64),
}

const CONFIRM_WINDOW: Duration = Duration::from_secs(5);
const AUTOSAVE_DELAY: Duration = Duration::from_millis(800);

pub fn run() -> anyhow::Result<()> {
    let ui = MainWindow::new()?;
    let fonts = Arc::new(FontCatalog::load_system());
    let engine = Arc::new(MenuEngine::new(fonts.clone()));

    let db_path = PresetStore::default_path();
    let (store, store_error) = match PresetStore::open(&db_path) {
        Ok(store) => (Some(store), None),
        Err(error) => {
            error!(path = %db_path.display(), error = %format!("{error:#}"), "could not open database; presets disabled");
            (None, Some(error))
        }
    };
    let restored = match store.as_ref().map(|s| s.load_last_document()) {
        Some(Ok(Some(doc))) => Some(doc),
        Some(Err(e)) => {
            warn!(error = %format!("{e:#}"), "autosaved document is unreadable; starting from the sample menu");
            None
        }
        _ => None,
    };
    let mut doc = match restored {
        Some(doc) => {
            info!(sections = doc.sections.len(), title = %doc.header.title, "restored autosaved document");
            doc
        }
        None => {
            info!("no autosaved document; starting with the sample menu");
            MenuDocument::sample()
        }
    };
    convert::normalize_fonts(&mut doc.style, &fonts);

    let settings = match store.as_ref().map(|s| s.load_app_settings()) {
        Some(Ok(Some(settings))) => {
            info!(theme = ?settings.theme, brand = %settings.brand_color.to_hex(), title = %settings.app_title, "app settings restored");
            settings
        }
        Some(Err(e)) => {
            warn!(error = %format!("{e:#}"), "app settings are unreadable; using defaults");
            AppSettings::default()
        }
        _ => {
            info!("no saved app settings; using the Casa Peix defaults");
            AppSettings::default()
        }
    };
    // Apply before the window is shown so it never flashes the wrong theme or language.
    branding::select_language(settings.language);
    engine.set_app_logo(settings.logo.clone());
    branding::apply(&ui, &settings);
    branding::fill_form(&ui, &settings);

    let controller = Controller::new(&ui, engine, store, doc, settings);
    controller.init_ui();
    controller.connect();
    match store_error {
        Some(e) => controller.status(
            StatusKind::Error,
            controller.tr(|t| t.invoke_database_unavailable(format!("{e:#}").into())),
        ),
        None => controller.status(StatusKind::Info, controller.tr(|t| t.invoke_ready())),
    }

    info!("main window shown");
    ui.run()?;
    info!("main window closed; saving document");
    controller.save_now();
    Ok(())
}

struct Controller {
    ui: slint::Weak<MainWindow>,
    editor: RefCell<EditorState>,
    engine: Arc<MenuEngine>,
    store: Option<PresetStore>,
    preview: PreviewWorker,
    autosave: Timer,
    resize: Timer,
    pending_confirm: RefCell<Option<(Confirm, Instant)>>,
    #[cfg(windows)]
    update_slot: update::UpdateSlot,
    settings: RefCell<AppSettings>,
}

impl Controller {
    fn new(
        ui: &MainWindow,
        engine: Arc<MenuEngine>,
        store: Option<PresetStore>,
        doc: MenuDocument,
        settings: AppSettings,
    ) -> Rc<Self> {
        let preview = PreviewWorker::spawn(engine.clone(), ui.as_weak());
        Rc::new(Self {
            ui: ui.as_weak(),
            editor: RefCell::new(EditorState::new(doc)),
            engine,
            store,
            preview,
            autosave: Timer::default(),
            resize: Timer::default(),
            pending_confirm: RefCell::new(None),
            settings: RefCell::new(settings),
            #[cfg(windows)]
            update_slot: Default::default(),
        })
    }

    fn ui(&self) -> MainWindow {
        self.ui
            .upgrade()
            .expect("main window alive while the controller runs")
    }

    fn fonts(&self) -> &FontCatalog {
        self.engine.fonts()
    }

    /// Runs `f` with the translated-messages global (see ui/strings.slint).
    fn tr<R>(&self, f: impl FnOnce(&Strings) -> R) -> R {
        let ui = self.ui();
        let strings = ui.global::<Strings>();
        f(&strings)
    }

    fn language(&self) -> menu_core::domain::Language {
        self.settings.borrow().language
    }

    /// Translated message for a logo that cannot be used.
    fn logo_error_message(&self, error: &LogoError) -> SharedString {
        self.tr(|t| match error {
            LogoError::Unreadable(detail) => t.invoke_image_unreadable(detail.as_str().into()),
            LogoError::UnsupportedFormat => t.invoke_image_unsupported(),
            LogoError::Invalid(detail) => t.invoke_image_invalid(detail.as_str().into()),
        })
    }

    // -----------------------------------------------------------------------
    // UI refresh helpers
    // -----------------------------------------------------------------------

    fn init_ui(self: &Rc<Self>) {
        let ui = self.ui();
        let catalog = ui.global::<Catalog>();
        catalog.set_font_families(convert::string_model(self.fonts().families()));
        about::fill(&ui);
        #[cfg(windows)]
        self.init_updates();
        self.apply_everything();
        self.refresh_presets();
    }

    /// Pushes the whole document (content + design) to the UI.
    fn apply_everything(self: &Rc<Self>) {
        let ui = self.ui();
        {
            let editor = self.editor.borrow();
            convert::apply_content(&ui, &editor);
            convert::apply_design(&ui, &editor.doc.style, self.fonts());
        }
        self.request_preview();
    }

    fn refresh_sections(&self) {
        let model = convert::sections_model(&self.editor.borrow());
        self.ui().global::<MenuState>().set_sections(model);
    }

    fn refresh_presets(&self) {
        let Some(store) = &self.store else { return };
        match store.list() {
            Ok(list) => self
                .ui()
                .global::<PresetState>()
                .set_presets(convert::preset_rows(&list)),
            Err(e) => self.status(
                StatusKind::Error,
                self.tr(|t| t.invoke_presets_unreadable(format!("{e:#}").into())),
            ),
        }
    }

    fn request_preview(&self) {
        let ui = self.ui();
        let doc = self.editor.borrow().doc.clone();
        let preview = ui.global::<PreviewState>();
        preview.set_page_label(convert::page_label(&ui, &doc.style, self.language()));
        let (w, h) = doc.style.page.size_pt();
        preview.set_aspect(h / w);
        preview.set_busy(true);
        // Render at the exact on-screen size so text stays crisp.
        let display_px = preview.get_display_width() * ui.window().scale_factor();
        let pixels_per_point = if display_px > 1.0 {
            display_px / w
        } else {
            1.5
        };
        self.preview.request(doc, pixels_per_point);
    }

    /// Shows a message in the status bar and records it in the log.
    fn status(&self, kind: StatusKind, message: impl AsRef<str>) {
        let message = message.as_ref();
        match kind {
            StatusKind::Info => debug!(status = %message, "status"),
            StatusKind::Success => info!(status = %message, "status"),
            StatusKind::Error => warn!(status = %message, "status"),
        }
        let ui = self.ui();
        let state = ui.global::<AppState>();
        state.set_status(message.into());
        state.set_status_kind(kind as i32);
    }

    /// Called after every edit: refreshes what is needed, re-renders the
    /// preview and schedules an autosave.
    fn changed(self: &Rc<Self>, structure: bool) {
        if structure {
            self.refresh_sections();
        }
        self.request_preview();
        let weak = Rc::downgrade(self);
        self.autosave
            .start(TimerMode::SingleShot, AUTOSAVE_DELAY, move || {
                if let Some(c) = weak.upgrade() {
                    c.save_now();
                }
            });
    }

    fn save_now(&self) {
        #[allow(clippy::collapsible_if)]
        if let Some(store) = &self.store {
            if let Err(e) = store.save_last_document(&self.editor.borrow().doc) {
                error!(error = %format!("{e:#}"), "autosave failed");
            }
        }
    }

    /// Returns true on the second click within the confirmation window.
    fn confirm(&self, action: Confirm, prompt: impl AsRef<str>) -> bool {
        let mut pending = self.pending_confirm.borrow_mut();
        if matches!(*pending, Some((a, at)) if a == action && at.elapsed() < CONFIRM_WINDOW) {
            *pending = None;
            return true;
        }
        *pending = Some((action, Instant::now()));
        drop(pending);
        self.status(StatusKind::Info, prompt);
        false
    }

    // -----------------------------------------------------------------------
    // Callback wiring
    // -----------------------------------------------------------------------

    fn connect(self: &Rc<Self>) {
        let ui = self.ui();
        let actions = ui.global::<Actions>();

        macro_rules! on {
            ($setter:ident, |$c:ident $(, $arg:ident : $ty:ty)*| $body:block) => {{
                let weak = Rc::downgrade(self);
                actions.$setter(move |$($arg: $ty),*| {
                    if let Some($c) = weak.upgrade() $body
                });
            }};
        }

        // --- Content -------------------------------------------------------
        on!(on_header_changed, |c| {
            convert::read_header(&c.ui(), &mut c.editor.borrow_mut().doc);
            c.changed(false);
        });
        on!(on_choose_logo, |c| {
            c.choose_logo();
        });
        on!(on_section_add, |c| {
            let name = c.tr(|t| t.invoke_new_section_name());
            c.editor.borrow_mut().add_section(&name);
            debug!(
                sections = c.editor.borrow().doc.sections.len(),
                "section added"
            );
            c.changed(true);
        });
        on!(on_section_remove, |c, si: i32| {
            c.remove_section(si as usize);
        });
        on!(on_section_move, |c, si: i32, delta: i32| {
            if c.editor.borrow_mut().move_section(si as usize, delta) {
                debug!(section = si, delta, "section moved");
                c.changed(true);
            }
        });
        on!(on_section_toggle, |c, si: i32| {
            c.editor.borrow_mut().toggle_section(si as usize);
            c.refresh_sections();
        });
        on!(
            on_section_edit,
            |c, si: i32, field: SharedString, value: SharedString| {
                if let Some(section) = c.editor.borrow_mut().doc.sections.get_mut(si as usize) {
                    match field.as_str() {
                        "name" => section.name = value.to_string(),
                        "note" => section.note = value.to_string(),
                        _ => {}
                    }
                    trace!(section = si, field = %field, "section edited");
                }
                c.changed(false);
            }
        );
        on!(on_product_add, |c, si: i32| {
            let name = c.tr(|t| t.invoke_new_dish_name());
            if c.editor.borrow_mut().add_product(si as usize, &name) {
                debug!(section = si, "dish added");
                c.changed(true);
            }
        });
        on!(on_product_remove, |c, si: i32, pi: i32| {
            if c.editor
                .borrow_mut()
                .remove_product(si as usize, pi as usize)
            {
                debug!(section = si, dish = pi, "dish removed");
                c.changed(true);
            }
        });
        on!(on_product_move, |c, si: i32, pi: i32, delta: i32| {
            if c.editor
                .borrow_mut()
                .move_product(si as usize, pi as usize, delta)
            {
                debug!(section = si, dish = pi, delta, "dish moved");
                c.changed(true);
            }
        });
        on!(on_product_toggle, |c, si: i32, pi: i32| {
            c.editor
                .borrow_mut()
                .toggle_product(si as usize, pi as usize);
            c.refresh_sections();
        });
        {
            let weak = Rc::downgrade(self);
            actions.on_product_edit(move |si, pi, field, value| {
                let Some(c) = weak.upgrade() else {
                    return SharedString::default();
                };
                let label = {
                    let mut editor = c.editor.borrow_mut();
                    let prices = editor.doc.style.prices.clone();
                    let Some(product) = editor.doc.product_mut(si as usize, pi as usize) else {
                        return SharedString::default();
                    };
                    match field.as_str() {
                        "name" => product.name = value.to_string(),
                        "description" => product.description = value.to_string(),
                        "price" => product.price = value.to_string(),
                        _ => {}
                    }
                    trace!(section = si, dish = pi, field = %field, "dish edited");
                    format_price(&product.price, &prices).unwrap_or_default()
                };
                c.changed(false);
                label.into()
            });
        }
        on!(on_allergen_toggle, |c, si: i32, pi: i32, ai: i32| {
            if c.editor
                .borrow_mut()
                .toggle_allergen(si as usize, pi as usize, ai as usize)
            {
                debug!(
                    section = si,
                    dish = pi,
                    allergen = ai + 1,
                    "allergen toggled"
                );
                c.changed(true);
            }
        });

        // --- Design --------------------------------------------------------
        on!(on_design_changed, |c| {
            let ui = c.ui();
            let prices_changed = {
                let mut editor = c.editor.borrow_mut();
                let before = editor.doc.style.prices.clone();
                convert::read_design(&ui, &mut editor.doc.style);
                convert::apply_color_swatches(&ui, &editor.doc.style);
                let layout = &editor.doc.style.layout;
                debug!(
                    columns = layout.columns,
                    align = ?layout.align,
                    page = %convert::page_summary(&editor.doc.style),
                    grow = editor.doc.style.fit.grow_to_fill,
                    "design changed"
                );
                before != editor.doc.style.prices
            };
            c.changed(prices_changed);
        });
        on!(
            on_font_changed,
            |c, row: i32, family: i32, size: i32, bold: bool, italic: bool| {
                let family_name = c.fonts().families().get(family.max(0) as usize).cloned();
                {
                    let mut editor = c.editor.borrow_mut();
                    if let Some(spec) =
                        convert::font_spec_mut(&mut editor.doc.style.typography, row as usize)
                    {
                        if let Some(name) = family_name {
                            spec.family = name;
                        }
                        spec.size_pt = size.clamp(4, 96) as f32;
                        spec.bold = bold;
                        spec.italic = italic;
                        debug!(row = convert::FONT_ROWS.get(row as usize).copied().unwrap_or("?"), family = %spec.family, size, bold, italic, "font changed");
                    }
                }
                c.changed(false);
            }
        );
        on!(on_reset_design, |c| {
            {
                let mut style = MenuStyle::default();
                convert::normalize_fonts(&mut style, c.fonts());
                // New designs follow the app's brand colour.
                style.palette.accent = c.settings.borrow().brand_color;
                c.editor.borrow_mut().doc.style = style;
            }
            convert::apply_design(&c.ui(), &c.editor.borrow().doc.style, c.fonts());
            c.changed(true);
            c.status(StatusKind::Success, c.tr(|t| t.invoke_design_reset()));
        });

        // --- Presets -------------------------------------------------------
        on!(on_preset_save_menu, |c, name: SharedString| {
            c.save_preset(&name, false);
        });
        on!(on_preset_save_style, |c, name: SharedString| {
            c.save_preset(&name, true);
        });
        on!(on_preset_load, |c, id: i32| {
            c.load_preset(id as i64);
        });
        on!(on_preset_delete, |c, id: i32| {
            c.delete_preset(id as i64);
        });
        on!(on_import_json, |c| {
            c.import_json();
        });
        on!(on_export_json, |c| {
            c.export_json();
        });

        // --- Document ------------------------------------------------------
        on!(on_new_document, |c| {
            c.new_document();
        });
        on!(on_export_pdf, |c| {
            c.export_pdf();
        });
        // --- About ---------------------------------------------------------
        on!(on_open_url, |c, url: SharedString| {
            if let Err(url) = about::open_url(&url) {
                c.status(
                    StatusKind::Error,
                    c.tr(|t| t.invoke_link_failed(url.into())),
                );
            }
        });
        // --- Updates (Windows only; elsewhere the UI hides them) -------------
        #[cfg(windows)]
        {
            on!(on_check_updates, |c| {
                c.check_for_updates(true);
            });
            on!(on_update_install, |c| {
                c.install_update();
            });
            on!(on_update_skip, |c| {
                c.skip_update();
            });
            on!(
                on_update_installed,
                |c, exe: SharedString, version: SharedString| {
                    c.finish_update(&exe, &version);
                }
            );
        }

        // --- Settings ------------------------------------------------------
        on!(on_settings_changed, |c| {
            let ui = c.ui();
            let (before, settings) = {
                let mut settings = c.settings.borrow_mut();
                let before = settings.clone();
                branding::read_form(&ui, &mut settings);
                (before, settings.clone())
            };
            debug!(language = settings.language.code(), theme = ?settings.theme, brand = %settings.brand_color.to_hex(), title = %settings.app_title, "app settings changed");
            c.apply_settings(&before, &settings);
            c.save_settings();
        });
        on!(on_choose_app_logo, |c| {
            c.choose_app_logo();
        });
        on!(on_reset_settings, |c| {
            let ui = c.ui();
            let defaults = AppSettings::default();
            let before = std::mem::replace(&mut *c.settings.borrow_mut(), defaults.clone());
            c.apply_settings(&before, &defaults);
            branding::fill_form(&ui, &defaults);
            c.save_settings();
            c.status(StatusKind::Success, c.tr(|t| t.invoke_settings_reset()));
        });

        on!(on_preview_resized, |c| {
            // Wait for the resize to settle before re-rendering.
            let weak = Rc::downgrade(&c);
            c.resize.start(
                TimerMode::SingleShot,
                Duration::from_millis(150),
                move || {
                    if let Some(c) = weak.upgrade() {
                        c.request_preview();
                    }
                },
            );
        });
    }

    // -----------------------------------------------------------------------
    // Actions
    // -----------------------------------------------------------------------

    /// Applies app settings, doing only the work that the change requires.
    fn apply_settings(&self, before: &AppSettings, settings: &AppSettings) {
        let ui = self.ui();
        if before.language != settings.language {
            branding::select_language(settings.language);
            // Rust-built texts are not re-evaluated by Slint: refresh them.
            self.request_preview();
            self.status(StatusKind::Info, self.tr(|t| t.invoke_ready()));
        }
        branding::apply(&ui, settings);
        if before.logo != settings.logo {
            // Menus using the default logo follow the app logo.
            self.engine.set_app_logo(settings.logo.clone());
            self.request_preview();
        }
    }

    fn save_settings(&self) {
        let Some(store) = &self.store else { return };
        if let Err(e) = store.save_app_settings(&self.settings.borrow()) {
            error!(error = %format!("{e:#}"), "could not save app settings");
            self.status(
                StatusKind::Error,
                self.tr(|t| t.invoke_settings_not_saved(format!("{e:#}").into())),
            );
        }
    }

    fn choose_app_logo(&self) {
        let ui = self.ui();
        let form = ui.global::<SettingsState>();
        let picked = rfd::FileDialog::new()
            .set_title(self.tr(|t| t.invoke_app_logo_dialog_title()).as_str())
            // Formats both the top bar (Slint) and the PDF (krilla) can use.
            .add_filter(
                self.tr(|t| t.invoke_images_filter()).as_str(),
                &["png", "jpg", "jpeg", "webp", "gif"],
            )
            .pick_file();
        let current = self.settings.borrow().logo.clone();
        let Some(path) = picked else {
            form.set_logo_mode(convert::logo_mode(&current));
            return;
        };
        // The app logo is also the default menu logo, so it must work in both places.
        let source = LogoSource::File(path.clone());
        let check = branding::load_logo_image(&path)
            .map_err(|_| LogoError::Unreadable(path.display().to_string()))
            .and_then(|_| self.engine.logo(&source).map(|_| ()));
        if let Err(error) = check {
            form.set_logo_mode(convert::logo_mode(&current));
            return self.status(StatusKind::Error, self.logo_error_message(&error));
        }
        info!(path = %path.display(), "app logo changed");
        form.set_logo_mode(1);
        form.set_logo_file_name(convert::logo_file_name(&source).into());
        let (before, settings) = {
            let mut settings = self.settings.borrow_mut();
            let before = settings.clone();
            settings.logo = source;
            (before, settings.clone())
        };
        self.apply_settings(&before, &settings);
        self.save_settings();
        self.status(
            StatusKind::Success,
            self.tr(|t| t.invoke_app_logo_updated()),
        );
    }

    fn choose_logo(self: &Rc<Self>) {
        let ui = self.ui();
        let picked = rfd::FileDialog::new()
            .set_title(self.tr(|t| t.invoke_menu_logo_dialog_title()).as_str())
            .add_filter(
                self.tr(|t| t.invoke_images_filter()).as_str(),
                &["png", "jpg", "jpeg", "webp", "gif"],
            )
            .pick_file();
        let current = self.editor.borrow().doc.header.logo.clone();
        let Some(path) = picked else {
            // Cancelled: restore the combo box to the current source.
            ui.global::<MenuState>()
                .set_logo_mode(convert::logo_mode(&current));
            return;
        };
        let source = LogoSource::File(path);
        match self.engine.logo(&source) {
            Ok(_) => {
                ui.global::<MenuState>()
                    .set_logo_file_name(convert::logo_file_name(&source).into());
                ui.global::<MenuState>().set_logo_mode(1);
                self.editor.borrow_mut().doc.header.logo = source;
                self.changed(false);
                self.status(StatusKind::Success, self.tr(|t| t.invoke_logo_updated()));
            }
            Err(e) => {
                ui.global::<MenuState>()
                    .set_logo_mode(convert::logo_mode(&current));
                self.status(StatusKind::Error, self.logo_error_message(&e));
            }
        }
    }

    fn remove_section(self: &Rc<Self>, si: usize) {
        let (name, count) = {
            let editor = self.editor.borrow();
            let Some(section) = editor.doc.sections.get(si) else {
                return;
            };
            (section.name.clone(), section.products.len())
        };
        if count > 0
            && !self.confirm(
                Confirm::DeleteSection(si),
                self.tr(|t| t.invoke_confirm_delete_section(name.as_str().into(), count as i32)),
            )
        {
            return;
        }
        if self.editor.borrow_mut().remove_section(si) {
            self.changed(true);
            self.status(
                StatusKind::Info,
                self.tr(|t| t.invoke_section_deleted(name.as_str().into())),
            );
        }
    }

    fn save_preset(self: &Rc<Self>, name: &str, style_only: bool) {
        let Some(store) = &self.store else {
            return self.status(StatusKind::Error, self.tr(|t| t.invoke_database_missing()));
        };
        let result = {
            let editor = self.editor.borrow();
            if style_only {
                store.save_style(name, &editor.doc.style)
            } else {
                store.save_menu(name, &editor.doc)
            }
        };
        match result {
            Ok(_) => {
                self.refresh_presets();
                let name: SharedString = name.trim().into();
                let message = self.tr(|t| {
                    if style_only {
                        t.invoke_design_preset_saved(name)
                    } else {
                        t.invoke_menu_preset_saved(name)
                    }
                });
                self.status(StatusKind::Success, message);
                self.ui().global::<PresetState>().set_new_name("".into());
            }
            Err(e) => self.status(
                StatusKind::Error,
                self.tr(|t| t.invoke_save_failed(format!("{e:#}").into())),
            ),
        }
    }

    fn load_preset(self: &Rc<Self>, id: i64) {
        let Some(store) = &self.store else { return };
        match store.load(id) {
            Ok(PresetPayload::Menu(doc)) => {
                self.load_document(*doc);
                self.status(StatusKind::Success, self.tr(|t| t.invoke_menu_loaded()));
            }
            Ok(PresetPayload::Style(style)) => {
                self.apply_style(*style);
                self.status(StatusKind::Success, self.tr(|t| t.invoke_design_applied()));
            }
            Err(e) => self.status(
                StatusKind::Error,
                self.tr(|t| t.invoke_load_failed(format!("{e:#}").into())),
            ),
        }
    }

    fn load_document(self: &Rc<Self>, mut doc: MenuDocument) {
        info!(
            title = %doc.header.title,
            sections = doc.sections.len(),
            dishes = doc.sections.iter().map(|s| s.products.len()).sum::<usize>(),
            "document loaded into the editor"
        );
        convert::normalize_fonts(&mut doc.style, self.fonts());
        self.editor.borrow_mut().replace(doc);
        self.apply_everything();
        self.changed(false);
    }

    fn apply_style(self: &Rc<Self>, mut style: MenuStyle) {
        info!(page = %convert::page_summary(&style), columns = style.layout.columns, "design applied");
        convert::normalize_fonts(&mut style, self.fonts());
        self.editor.borrow_mut().doc.style = style;
        convert::apply_design(&self.ui(), &self.editor.borrow().doc.style, self.fonts());
        self.changed(true);
    }

    fn delete_preset(self: &Rc<Self>, id: i64) {
        let Some(store) = &self.store else { return };
        let name = store
            .list()
            .ok()
            .and_then(|l| l.into_iter().find(|p| p.id == id))
            .map(|p| p.name);
        let name = name.unwrap_or_default();
        if !self.confirm(
            Confirm::DeletePreset(id),
            self.tr(|t| t.invoke_confirm_delete_preset(name.as_str().into())),
        ) {
            return;
        }
        match store.delete(id) {
            Ok(()) => {
                self.refresh_presets();
                self.status(
                    StatusKind::Info,
                    self.tr(|t| t.invoke_preset_deleted(name.as_str().into())),
                );
            }
            Err(e) => self.status(
                StatusKind::Error,
                self.tr(|t| t.invoke_delete_failed(format!("{e:#}").into())),
            ),
        }
    }

    fn import_json(self: &Rc<Self>) {
        let Some(path) = rfd::FileDialog::new()
            .set_title(self.tr(|t| t.invoke_import_dialog_title()).as_str())
            .add_filter(self.tr(|t| t.invoke_json_filter()).as_str(), &["json"])
            .pick_file()
        else {
            return;
        };
        info!(path = %path.display(), "importing JSON");
        let text = match std::fs::read_to_string(&path) {
            Ok(t) => t,
            Err(e) => {
                return self.status(
                    StatusKind::Error,
                    self.tr(|t| t.invoke_file_unreadable(e.to_string().into())),
                );
            }
        };
        let value: serde_json::Value = match serde_json::from_str(&text) {
            Ok(v) => v,
            Err(e) => {
                return self.status(
                    StatusKind::Error,
                    self.tr(|t| t.invoke_invalid_json(e.to_string().into())),
                );
            }
        };
        // A full menu has "sections"; otherwise treat it as a design.
        if value.get("sections").is_some() || value.get("header").is_some() {
            match serde_json::from_value::<MenuDocument>(value) {
                Ok(doc) => {
                    self.load_document(doc);
                    self.status(StatusKind::Success, self.tr(|t| t.invoke_menu_imported()));
                }
                Err(e) => self.status(
                    StatusKind::Error,
                    self.tr(|t| t.invoke_invalid_menu(e.to_string().into())),
                ),
            }
        } else {
            match serde_json::from_value::<MenuStyle>(value) {
                Ok(style) => {
                    self.apply_style(style);
                    self.status(StatusKind::Success, self.tr(|t| t.invoke_design_imported()));
                }
                Err(e) => self.status(
                    StatusKind::Error,
                    self.tr(|t| t.invoke_invalid_design(e.to_string().into())),
                ),
            }
        }
    }

    fn export_json(&self) {
        let doc = self.editor.borrow().doc.clone();
        let Some(path) = rfd::FileDialog::new()
            .set_title(self.tr(|t| t.invoke_export_json_dialog_title()).as_str())
            .set_file_name(format!("{}.json", file_stem(&doc.header.title)))
            .add_filter(self.tr(|t| t.invoke_json_filter()).as_str(), &["json"])
            .save_file()
        else {
            return;
        };
        let path = with_extension(path, "json");
        info!(path = %path.display(), "exporting menu to JSON");
        let result = serde_json::to_string_pretty(&doc)
            .map_err(anyhow::Error::from)
            .and_then(|json| std::fs::write(&path, json).map_err(anyhow::Error::from));
        match result {
            Ok(()) => self.status(
                StatusKind::Success,
                self.tr(|t| t.invoke_menu_exported(path.display().to_string().into())),
            ),
            Err(e) => self.status(
                StatusKind::Error,
                self.tr(|t| t.invoke_export_failed(format!("{e:#}").into())),
            ),
        }
    }

    fn new_document(self: &Rc<Self>) {
        if !self.confirm(
            Confirm::NewDocument,
            self.tr(|t| t.invoke_confirm_new_document()),
        ) {
            return;
        }
        let style = self.editor.borrow().doc.style.clone();
        let mut doc = MenuDocument::blank(&self.tr(|t| t.invoke_new_section_name()));
        doc.style = style;
        self.load_document(doc);
        self.status(
            StatusKind::Info,
            self.tr(|t| t.invoke_new_document_created()),
        );
    }

    fn export_pdf(&self) {
        let doc = self.editor.borrow().doc.clone();
        let Some(path) = rfd::FileDialog::new()
            .set_title(self.tr(|t| t.invoke_pdf_dialog_title()).as_str())
            .set_file_name(format!("{}.pdf", file_stem(&doc.header.title)))
            .add_filter("PDF", &["pdf"])
            .save_file()
        else {
            return;
        };
        let path = with_extension(path, "pdf");
        info!(path = %path.display(), "exporting PDF");
        let result = self.engine.render(&doc).and_then(|r| {
            std::fs::write(&path, &r.pdf)
                .map(|_| r)
                .map_err(anyhow::Error::from)
        });
        if let Ok(r) = &result {
            info!(
                path = %path.display(),
                bytes = r.pdf.len(),
                scale = r.scale,
                fits = r.fits,
                page = %convert::page_summary(&doc.style),
                "PDF exported"
            );
        }
        match result {
            Ok(r) if !r.fits => self.status(
                StatusKind::Error,
                self.tr(|t| t.invoke_pdf_saved_overflow(path.display().to_string().into())),
            ),
            Ok(_) => self.status(
                StatusKind::Success,
                self.tr(|t| t.invoke_pdf_saved(path.display().to_string().into())),
            ),
            Err(e) => self.status(
                StatusKind::Error,
                self.tr(|t| t.invoke_pdf_failed(format!("{e:#}").into())),
            ),
        }
    }
}

/// File-name friendly version of the menu title ("Carta del Jardín" → "carta-del-jardin").
fn file_stem(title: &str) -> String {
    let mut out = String::new();
    for ch in title.trim().to_lowercase().chars() {
        let mapped = match ch {
            'á' | 'à' | 'ä' | 'â' => 'a',
            'é' | 'è' | 'ë' | 'ê' => 'e',
            'í' | 'ì' | 'ï' | 'î' => 'i',
            'ó' | 'ò' | 'ö' | 'ô' => 'o',
            'ú' | 'ù' | 'ü' | 'û' => 'u',
            'ñ' => 'n',
            'ç' => 'c',
            c if c.is_ascii_alphanumeric() => c,
            _ => '-',
        };
        if mapped == '-' && (out.is_empty() || out.ends_with('-')) {
            continue;
        }
        out.push(mapped);
    }
    let out = out.trim_end_matches('-').to_string();
    if out.is_empty() { "menu".into() } else { out }
}

fn with_extension(path: PathBuf, ext: &str) -> PathBuf {
    if path
        .extension()
        .is_some_and(|e| e.eq_ignore_ascii_case(ext))
    {
        path
    } else {
        path.with_extension(ext)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn file_stems() {
        assert_eq!(file_stem("Carta del Jardín"), "carta-del-jardin");
        assert_eq!(file_stem("  ¡Menú!  "), "menu");
        assert_eq!(file_stem(""), "menu");
    }
}
