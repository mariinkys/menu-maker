//! Tracing setup for the binary.
//!
//! Events go to two places:
//! * stderr (coloured, for development),
//! * `<data dir>/logs/menu-maker.YYYY-MM-DD.log` (rotated daily, last 7 kept),
//!   which is the only place to look on Windows release builds, where there
//!   is no console.
//!
//! The level is `info` for this application and `warn` for dependencies.
//! Override it with `RUST_LOG`, e.g. `RUST_LOG=menu_core=debug,menu_maker=trace`.

use std::path::Path;

use tracing_appender::non_blocking::WorkerGuard;
use tracing_appender::rolling::{Builder, Rotation};
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;
use tracing_subscriber::{EnvFilter, Layer, fmt};

use menu_core::paths::APP_NAME;

const DEFAULT_FILTER: &str = "warn,menu_core=info,menu_maker=info";
const KEEP_LOG_FILES: usize = 7;

/// Installs the global subscriber. Keep the returned guard alive until the
/// end of `main` so buffered file output is flushed on exit.
pub fn init(log_dir: &Path) -> Option<WorkerGuard> {
    let filter = || EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new(DEFAULT_FILTER));
    let stderr_layer = fmt::layer().with_writer(std::io::stderr).with_target(true).with_filter(filter());

    let file_appender = std::fs::create_dir_all(log_dir).ok().and_then(|_| {
        Builder::new()
            .rotation(Rotation::DAILY)
            .filename_prefix(APP_NAME)
            .filename_suffix("log")
            .max_log_files(KEEP_LOG_FILES)
            .build(log_dir)
            .ok()
    });

    let (file_layer, guard) = match file_appender {
        Some(appender) => {
            let (writer, guard) = tracing_appender::non_blocking(appender);
            let layer = fmt::layer().with_writer(writer).with_ansi(false).with_target(true).with_filter(filter());
            (Some(layer), Some(guard))
        }
        None => (None, None),
    };

    tracing_subscriber::registry().with(stderr_layer).with(file_layer).init();
    if guard.is_none() {
        tracing::warn!(dir = %log_dir.display(), "could not create the log folder; logging to stderr only");
    }
    install_panic_hook();
    guard
}

/// Routes panics through tracing so they end up in the log file too.
fn install_panic_hook() {
    let default_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let location = info.location().map(|l| format!("{}:{}", l.file(), l.line())).unwrap_or_default();
        let message = info
            .payload()
            .downcast_ref::<&str>()
            .map(|s| s.to_string())
            .or_else(|| info.payload().downcast_ref::<String>().cloned())
            .unwrap_or_else(|| "unknown panic".into());
        tracing::error!(%location, %message, "application panicked");
        default_hook(info);
    }));
}
