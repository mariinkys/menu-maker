//! Languages supported by the interface and by the printed menu texts.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Language {
    /// Spanish (Spain).
    #[default]
    Es,
    /// English. Also the source language of the UI strings.
    En,
}

impl Language {
    pub const ALL: [Language; 2] = [Language::Es, Language::En];

    /// Locale code: the folder name under `translations/` for Slint, `en` for the source strings.
    pub fn code(self) -> &'static str {
        match self {
            Language::Es => "es",
            Language::En => "en",
        }
    }

    pub fn index(self) -> usize {
        Self::ALL.iter().position(|l| *l == self).unwrap_or(0)
    }

    pub fn from_index(i: usize) -> Self {
        Self::ALL.get(i).copied().unwrap_or_default()
    }
}
