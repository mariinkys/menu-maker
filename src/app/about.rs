//! About dialog: package metadata (from Cargo.toml, at compile time) and
//! opening links in the system browser.

use slint::ComponentHandle;
use tracing::{info, warn};

use crate::{AboutInfo, MainWindow};

/// SPDX license page, e.g. `GPL-3.0` → spdx.org/licenses/GPL-3.0.html.
pub fn license_url(license: &str) -> String {
    format!("https://spdx.org/licenses/{}.html", license.trim())
}

/// Cargo joins multiple authors with ':'.
pub fn format_authors(authors: &str) -> String {
    authors
        .split(':')
        .map(str::trim)
        .filter(|a| !a.is_empty())
        .collect::<Vec<_>>()
        .join(", ")
}

pub fn fill(ui: &MainWindow) {
    let about = ui.global::<AboutInfo>();
    about.set_name(env!("CARGO_PKG_NAME").into());
    about.set_version(env!("CARGO_PKG_VERSION").into());
    about.set_description(env!("CARGO_PKG_DESCRIPTION").into());
    about.set_license(env!("CARGO_PKG_LICENSE").into());
    about.set_license_url(license_url(env!("CARGO_PKG_LICENSE")).into());
    about.set_authors(format_authors(env!("CARGO_PKG_AUTHORS")).into());
    about.set_repository(env!("CARGO_PKG_REPOSITORY").into());
}

/// Opens `url` with the default browser. Only http(s) links are accepted.
pub fn open_url(url: &str) -> Result<(), String> {
    if !(url.starts_with("https://") || url.starts_with("http://")) {
        warn!(url, "refusing to open a non-web link");
        return Err(url.to_string());
    }
    info!(url, "opening link in the browser");
    open::that_detached(url).map_err(|e| {
        warn!(url, error = %e, "could not open link");
        url.to_string()
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn metadata_helpers() {
        assert_eq!(
            license_url("GPL-3.0"),
            "https://spdx.org/licenses/GPL-3.0.html"
        );
        assert_eq!(format_authors("Ana <a@x.es>:Bob"), "Ana <a@x.es>, Bob");
        assert!(open_url("file:///etc/passwd").is_err());
    }
}
