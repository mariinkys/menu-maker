//! SQLite persistence: named presets and the last working document.
//!
//! Documents are stored as JSON so the schema does not need to change every
//! time a new style option is added (`serde(default)` handles old rows).

use std::path::{Path, PathBuf};

use anyhow::Context;
use rusqlite::{Connection, OptionalExtension, params};
use tracing::{debug, info};

use crate::domain::{MenuDocument, MenuStyle};
use crate::settings::AppSettings;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PresetKind {
    /// Full menu: content and design.
    Menu,
    /// Design only (page, layout, fonts, colours…).
    Style,
}

impl PresetKind {
    fn as_str(self) -> &'static str {
        match self {
            PresetKind::Menu => "menu",
            PresetKind::Style => "style",
        }
    }

    fn parse(s: &str) -> Self {
        if s == "style" {
            PresetKind::Style
        } else {
            PresetKind::Menu
        }
    }
}

#[derive(Debug, Clone)]
pub struct PresetSummary {
    pub id: i64,
    pub name: String,
    pub kind: PresetKind,
    pub updated_at: String,
}

pub enum PresetPayload {
    Menu(Box<MenuDocument>),
    Style(Box<MenuStyle>),
}

pub struct PresetStore {
    conn: Connection,
}

const MIGRATIONS: &[&str] = &[
    // v1
    "CREATE TABLE presets (
        id          INTEGER PRIMARY KEY AUTOINCREMENT,
        name        TEXT NOT NULL,
        kind        TEXT NOT NULL CHECK (kind IN ('menu', 'style')),
        payload     TEXT NOT NULL,
        created_at  TEXT NOT NULL DEFAULT (datetime('now', 'localtime')),
        updated_at  TEXT NOT NULL DEFAULT (datetime('now', 'localtime')),
        UNIQUE (name, kind)
    );
    CREATE TABLE app_state (
        key   TEXT PRIMARY KEY,
        value TEXT NOT NULL
    );",
];

const LAST_DOCUMENT_KEY: &str = "last_document";
const APP_SETTINGS_KEY: &str = "app_settings";

impl PresetStore {
    /// Default location, see [`crate::paths::database_path`].
    pub fn default_path() -> PathBuf {
        crate::paths::database_path()
    }

    pub fn open(path: &Path) -> anyhow::Result<Self> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)
                .with_context(|| format!("could not create {}", dir.display()))?;
        }
        let conn =
            Connection::open(path).with_context(|| format!("could not open {}", path.display()))?;
        let store = Self::init(conn)?;
        info!(path = %path.display(), "database opened");
        Ok(store)
    }

    pub fn open_in_memory() -> anyhow::Result<Self> {
        Self::init(Connection::open_in_memory()?)
    }

    fn init(conn: Connection) -> anyhow::Result<Self> {
        conn.pragma_update(None, "journal_mode", "WAL").ok();
        let version: i64 = conn.pragma_query_value(None, "user_version", |r| r.get(0))?;
        for (i, sql) in MIGRATIONS.iter().enumerate().skip(version as usize) {
            info!(from = i, to = i + 1, "applying database migration");
            conn.execute_batch(sql)
                .with_context(|| format!("migration {} failed", i + 1))?;
            conn.pragma_update(None, "user_version", (i + 1) as i64)?;
        }
        debug!(
            schema_version = MIGRATIONS.len(),
            "database schema up to date"
        );
        Ok(Self { conn })
    }

    pub fn list(&self) -> anyhow::Result<Vec<PresetSummary>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, name, kind, updated_at FROM presets ORDER BY kind, name COLLATE NOCASE",
        )?;
        let rows = stmt.query_map([], |r| {
            Ok(PresetSummary {
                id: r.get(0)?,
                name: r.get(1)?,
                kind: PresetKind::parse(&r.get::<_, String>(2)?),
                updated_at: r.get(3)?,
            })
        })?;
        Ok(rows.collect::<Result<_, _>>()?)
    }

    /// Inserts or overwrites the preset with the same name and kind.
    pub fn save_menu(&self, name: &str, doc: &MenuDocument) -> anyhow::Result<i64> {
        self.upsert(name, PresetKind::Menu, &serde_json::to_string(doc)?)
    }

    pub fn save_style(&self, name: &str, style: &MenuStyle) -> anyhow::Result<i64> {
        self.upsert(name, PresetKind::Style, &serde_json::to_string(style)?)
    }

    fn upsert(&self, name: &str, kind: PresetKind, payload: &str) -> anyhow::Result<i64> {
        let name = name.trim();
        anyhow::ensure!(!name.is_empty(), "the preset name is empty");
        self.conn.execute(
            "INSERT INTO presets (name, kind, payload) VALUES (?1, ?2, ?3)
             ON CONFLICT (name, kind) DO UPDATE
               SET payload = excluded.payload, updated_at = datetime('now', 'localtime')",
            params![name, kind.as_str(), payload],
        )?;
        let id = self.conn.query_row(
            "SELECT id FROM presets WHERE name = ?1 AND kind = ?2",
            params![name, kind.as_str()],
            |r| r.get(0),
        )?;
        info!(
            id,
            name,
            kind = kind.as_str(),
            bytes = payload.len(),
            "preset saved"
        );
        Ok(id)
    }

    pub fn load(&self, id: i64) -> anyhow::Result<PresetPayload> {
        let (kind, payload): (String, String) = self
            .conn
            .query_row(
                "SELECT kind, payload FROM presets WHERE id = ?1",
                [id],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .context("the preset does not exist")?;
        info!(id, kind, "preset loaded");
        Ok(match PresetKind::parse(&kind) {
            PresetKind::Menu => PresetPayload::Menu(Box::new(serde_json::from_str(&payload)?)),
            PresetKind::Style => PresetPayload::Style(Box::new(serde_json::from_str(&payload)?)),
        })
    }

    pub fn rename(&self, id: i64, new_name: &str) -> anyhow::Result<()> {
        let new_name = new_name.trim();
        anyhow::ensure!(!new_name.is_empty(), "the preset name is empty");
        self.conn
            .execute(
                "UPDATE presets SET name = ?1 WHERE id = ?2",
                params![new_name, id],
            )
            .context("a preset with that name already exists")?;
        info!(id, new_name, "preset renamed");
        Ok(())
    }

    pub fn delete(&self, id: i64) -> anyhow::Result<()> {
        let removed = self
            .conn
            .execute("DELETE FROM presets WHERE id = ?1", [id])?;
        info!(id, removed, "preset deleted");
        Ok(())
    }

    /// Autosave of the document being edited, restored on next start.
    pub fn save_last_document(&self, doc: &MenuDocument) -> anyhow::Result<()> {
        self.conn.execute(
            "INSERT INTO app_state (key, value) VALUES (?1, ?2)
             ON CONFLICT (key) DO UPDATE SET value = excluded.value",
            params![LAST_DOCUMENT_KEY, serde_json::to_string(doc)?],
        )?;
        debug!(sections = doc.sections.len(), "document autosaved");
        Ok(())
    }

    pub fn load_last_document(&self) -> anyhow::Result<Option<MenuDocument>> {
        let json: Option<String> = self
            .conn
            .query_row(
                "SELECT value FROM app_state WHERE key = ?1",
                [LAST_DOCUMENT_KEY],
                |r| r.get(0),
            )
            .optional()?;
        Ok(match json {
            Some(json) => Some(serde_json::from_str(&json)?),
            None => None,
        })
    }
}

impl PresetStore {
    fn set_state(&self, key: &str, value: &str) -> anyhow::Result<()> {
        self.conn.execute(
            "INSERT INTO app_state (key, value) VALUES (?1, ?2)
             ON CONFLICT (key) DO UPDATE SET value = excluded.value",
            params![key, value],
        )?;
        Ok(())
    }

    fn get_state(&self, key: &str) -> anyhow::Result<Option<String>> {
        Ok(self
            .conn
            .query_row("SELECT value FROM app_state WHERE key = ?1", [key], |r| {
                r.get(0)
            })
            .optional()?)
    }

    pub fn save_app_settings(&self, settings: &AppSettings) -> anyhow::Result<()> {
        self.set_state(APP_SETTINGS_KEY, &serde_json::to_string(settings)?)?;
        info!(theme = ?settings.theme, brand = %settings.brand_color.to_hex(), title = %settings.app_title, logo = ?settings.logo, "app settings saved");
        Ok(())
    }

    /// Stored settings, or `None` on first run.
    pub fn load_app_settings(&self) -> anyhow::Result<Option<AppSettings>> {
        Ok(match self.get_state(APP_SETTINGS_KEY)? {
            Some(json) => Some(serde_json::from_str(&json)?),
            None => None,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn save_list_load_delete() {
        let store = PresetStore::open_in_memory().unwrap();
        let doc = MenuDocument::sample();
        let id = store.save_menu("Verano", &doc).unwrap();
        // Same name + kind overwrites.
        assert_eq!(store.save_menu("Verano", &doc).unwrap(), id);
        let style_id = store.save_style("Verano", &doc.style).unwrap();
        assert_ne!(id, style_id);
        assert_eq!(store.list().unwrap().len(), 2);

        match store.load(id).unwrap() {
            PresetPayload::Menu(loaded) => assert_eq!(*loaded, doc),
            PresetPayload::Style(_) => panic!("wrong kind"),
        }
        store.rename(id, "Verano 2027").unwrap();
        assert!(
            store
                .list()
                .unwrap()
                .iter()
                .any(|p| p.name == "Verano 2027")
        );
        store.delete(id).unwrap();
        assert_eq!(store.list().unwrap().len(), 1);
        assert!(store.save_menu("  ", &doc).is_err());
    }

    #[test]
    fn last_document_roundtrip() {
        let store = PresetStore::open_in_memory().unwrap();
        assert!(store.load_last_document().unwrap().is_none());
        let doc = MenuDocument::sample();
        store.save_last_document(&doc).unwrap();
        assert_eq!(store.load_last_document().unwrap().unwrap(), doc);
    }

    #[test]
    fn app_settings_roundtrip() {
        let store = PresetStore::open_in_memory().unwrap();
        assert!(store.load_app_settings().unwrap().is_none());
        let settings = AppSettings {
            theme: crate::settings::ThemeMode::Dark,
            app_title: "La Trattoria".into(),
            ..Default::default()
        };
        store.save_app_settings(&settings).unwrap();
        assert_eq!(store.load_app_settings().unwrap().unwrap(), settings);
    }
}
