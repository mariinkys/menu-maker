//! Where the application keeps its files, resolved with the `dirs` crate.

use std::path::PathBuf;

/// Reverse-DNS application identifier, used as the data folder name.
pub const APP_ID: &str = "dev.mariinkys.MenuMaker";
/// Short application name (binary, log file prefix, database file).
pub const APP_NAME: &str = "menu-maker";

/// Root folder for all application data. Falls back to `./dev.mariinkys.MenuMaker`
/// when the OS does not report a data directory.
pub fn data_dir() -> PathBuf {
    dirs::data_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(APP_ID)
}

/// SQLite database with presets and the autosaved document.
pub fn database_path() -> PathBuf {
    data_dir().join(format!("{APP_NAME}.sqlite3"))
}

/// Folder for the rolling log files.
pub fn log_dir() -> PathBuf {
    data_dir().join("logs")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn paths_live_under_the_app_folder() {
        let root = data_dir();
        assert!(root.ends_with(APP_ID));
        assert!(database_path().starts_with(&root));
        assert!(log_dir().starts_with(&root));
        assert_eq!(database_path().file_name().unwrap(), "menu-maker.sqlite3");
    }
}
