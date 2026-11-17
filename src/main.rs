#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

slint::include_modules!();

mod app;
mod logging;

use menu_core::paths;

fn main() -> anyhow::Result<()> {
    let _log_guard = logging::init(&paths::log_dir());
    tracing::info!(
        version = env!("CARGO_PKG_VERSION"),
        os = std::env::consts::OS,
        data_dir = %paths::data_dir().display(),
        log_dir = %paths::log_dir().display(),
        "starting {}",
        paths::APP_NAME
    );
    let result = app::run();
    match &result {
        Ok(()) => tracing::info!("exited normally"),
        Err(e) => tracing::error!(error = %format!("{e:#}"), "exited with an error"),
    }
    result
}
