//! Auto-update flow (Windows): background check at startup, update dialog,
//! download with progress, replace the exe and restart. The mechanics
//! (GitHub API, checksum, self-replace) live in `menu_core::updater`.

use std::rc::Rc;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use slint::ComponentHandle;
use tracing::{error, info, warn};

use menu_core::updater::{self, AvailableUpdate, UpdateSource};
use semver::Version;

use super::Controller;
use crate::{Actions, UpdateState};

/// Delay before the automatic check, so startup stays fast.
const STARTUP_CHECK_DELAY: Duration = Duration::from_secs(3);

/// The update found by the last check, shared with the worker threads.
pub type UpdateSlot = Arc<Mutex<Option<AvailableUpdate>>>;

impl Controller {
    pub(super) fn init_updates(self: &Rc<Self>) {
        let ui = self.ui();
        let state = ui.global::<UpdateState>();
        // Only compiled on Windows; elsewhere `supported` stays false and the UI hides updates.
        state.set_supported(true);
        state.set_current_version(env!("CARGO_PKG_VERSION").into());
        if self.settings.borrow().check_updates {
            let weak = Rc::downgrade(self);
            slint::Timer::single_shot(STARTUP_CHECK_DELAY, move || {
                if let Some(c) = weak.upgrade() {
                    c.check_for_updates(false);
                }
            });
        }
    }

    /// `manual`: started from the About dialog. Then the dialog shows
    /// "checking…" and "up to date"; automatic checks stay silent unless
    /// there is something new that the user hasn't skipped.
    pub(super) fn check_for_updates(&self, manual: bool) {
        let Some(mut source) = UpdateSource::from_package() else {
            warn!("repository in Cargo.toml is not a GitHub URL; updates disabled");
            return;
        };
        // If the last update installed a version newer than this exe reports
        // (Cargo.toml not bumped for that release), compare against that one.
        if let Ok(installed) = Version::parse(&self.settings.borrow().installed_version) {
            if installed > source.current {
                info!(%installed, reported = %source.current, "using the last installed version for update checks");
                source.current = installed;
            }
        }
        let ui = self.ui();
        let state = ui.global::<UpdateState>();
        if manual {
            state.set_phase(0);
            state.set_open(true);
        }
        let skipped = self.settings.borrow().skipped_version.clone();
        let slot = self.update_slot.clone();
        let weak = ui.as_weak();

        std::thread::spawn(move || {
            let result = updater::check(&source);
            let can_install = updater::can_self_replace();
            let _ = slint::invoke_from_event_loop(move || {
                let Some(ui) = weak.upgrade() else { return };
                let state = ui.global::<UpdateState>();
                match result {
                    Ok(Some(update)) => {
                        let version = update.version.to_string();
                        if !manual && version == skipped {
                            info!(%version, "update available but skipped by the user");
                            return;
                        }
                        state.set_new_version(version.into());
                        state.set_notes(update.notes.as_str().into());
                        state.set_page_url(update.page_url.as_str().into());
                        state.set_can_install(can_install && update.can_verify());
                        state.set_phase(1);
                        state.set_open(true);
                        *slot.lock().unwrap() = Some(update);
                    }
                    Ok(None) if manual => state.set_phase(5),
                    Ok(None) => {}
                    Err(e) => {
                        // Offline or rate-limited: never bother the user at startup.
                        warn!(error = %format!("{e:#}"), "update check failed");
                        if manual {
                            state.set_error(format!("{e:#}").into());
                            state.set_phase(6);
                        }
                    }
                }
            });
        });
    }

    pub(super) fn install_update(&self) {
        let Some(update) = self.update_slot.lock().unwrap().clone() else {
            return;
        };
        // Make sure the new instance opens the current work.
        self.save_now();

        let ui = self.ui();
        let state = ui.global::<UpdateState>();
        state.set_progress(0.0);
        state.set_phase(2);
        let weak = ui.as_weak();

        std::thread::spawn(move || {
            let progress_ui = weak.clone();
            let mut last_percent = -1;
            let result = updater::download_and_verify(&update, |done, total| {
                let percent = (done * 100 / total.max(1)) as i32;
                if percent != last_percent {
                    last_percent = percent;
                    let ui = progress_ui.clone();
                    let _ = slint::invoke_from_event_loop(move || {
                        if let Some(ui) = ui.upgrade() {
                            ui.global::<UpdateState>()
                                .set_progress(percent as f32 / 100.0);
                        }
                    });
                }
            })
            .and_then(|new_exe| updater::install(&new_exe));
            let version = update.version.to_string();

            let _ = slint::invoke_from_event_loop(move || {
                let Some(ui) = weak.upgrade() else { return };
                match result {
                    // Hand over to the controller (see `finish_update`).
                    Ok(exe) => ui
                        .global::<Actions>()
                        .invoke_update_installed(exe.display().to_string().into(), version.into()),
                    Err(e) => {
                        error!(error = %format!("{e:#}"), "update failed");
                        let state = ui.global::<UpdateState>();
                        state.set_error(format!("{e:#}").into());
                        state.set_phase(4);
                    }
                }
            });
        });
    }

    /// The exe was replaced: remember the version, save, start the new
    /// version and close this one.
    pub(super) fn finish_update(&self, exe: &str, version: &str) {
        self.settings.borrow_mut().installed_version = version.to_string();
        self.save_settings();
        self.save_now();
        let ui = self.ui();
        let state = ui.global::<UpdateState>();
        match updater::relaunch(std::path::Path::new(exe)) {
            Ok(()) => {
                state.set_phase(3);
                info!(%version, "update installed; closing this instance");
                // `run()` returns, saves once more and exits.
                let _ = slint::quit_event_loop();
            }
            Err(e) => {
                error!(error = %format!("{e:#}"), "could not restart after updating");
                state.set_error(format!("{e:#}").into());
                state.set_phase(4);
            }
        }
    }

    pub(super) fn skip_update(&self) {
        let ui = self.ui();
        let version = ui.global::<UpdateState>().get_new_version().to_string();
        info!(%version, "user skipped this version");
        self.settings.borrow_mut().skipped_version = version;
        self.save_settings();
        ui.global::<UpdateState>().set_open(false);
    }
}
