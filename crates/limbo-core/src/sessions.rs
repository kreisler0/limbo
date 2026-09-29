//! Session save/restore. On startup only the active tab gets a webview; the
//! rest come back DISCARDED (title, favicon and snapshot only).

use rusqlite::{Connection, OptionalExtension, params};
use serde::{Deserialize, Serialize};

use crate::error::Result;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct SessionTab {
    pub url: String,
    pub title: String,
    pub favicon_url: Option<String>,
    pub pinned: bool,
    pub muted: bool,
    /// Page zoom factor, 1.0 = 100%.
    pub zoom: f64,
    /// File name of the last snapshot (JPEG) in the snapshots folder.
    pub snapshot: Option<String>,
}

impl Default for SessionTab {
    fn default() -> Self {
        SessionTab {
            url: String::new(),
            title: String::new(),
            favicon_url: None,
            pinned: false,
            muted: false,
            zoom: 1.0,
            snapshot: None,
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Session {
    pub tabs: Vec<SessionTab>,
    pub active: usize,
    /// Window placement: x, y, width, height (physical pixels) and maximized.
    pub window: Option<WindowPlacement>,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WindowPlacement {
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
    pub maximized: bool,
}

impl Session {
    /// Keeps the session sane: at most `max` tabs, a valid active index,
    /// pinned tabs first.
    pub fn normalized(mut self, max: usize) -> Session {
        let active_url = self.tabs.get(self.active).map(|t| t.url.clone());
        self.tabs.retain(|t| !t.url.is_empty());
        self.tabs.sort_by_key(|t| !t.pinned);
        self.tabs.truncate(max);
        self.active = active_url
            .and_then(|u| self.tabs.iter().position(|t| t.url == u))
            .unwrap_or(0)
            .min(self.tabs.len().saturating_sub(1));
        self
    }
}

pub fn save(conn: &Connection, session: &Session, now_us: i64) -> Result<()> {
    let json = serde_json::to_string(session)?;
    conn.execute(
        "INSERT INTO session(id, data, saved_us) VALUES (1, ?1, ?2)
         ON CONFLICT(id) DO UPDATE SET data = excluded.data, saved_us = excluded.saved_us",
        params![json, now_us],
    )?;
    Ok(())
}

pub fn load(conn: &Connection) -> Result<Option<Session>> {
    let json: Option<String> = conn.query_row("SELECT data FROM session WHERE id = 1", [], |r| r.get(0)).optional()?;
    Ok(json.and_then(|j| serde_json::from_str(&j).ok()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::Db;

    fn tab(url: &str, pinned: bool) -> SessionTab {
        SessionTab { url: url.into(), title: url.into(), pinned, ..Default::default() }
    }

    #[test]
    fn save_load_roundtrip() {
        let db = Db::open_in_memory().unwrap();
        assert_eq!(load(db.conn()).unwrap(), None);
        let s = Session {
            tabs: vec![tab("https://a.com/", false), tab("https://b.com/", true)],
            active: 1,
            window: Some(WindowPlacement { x: 10, y: 20, width: 1280, height: 800, maximized: true }),
        };
        save(db.conn(), &s, 1).unwrap();
        save(db.conn(), &s, 2).unwrap();
        assert_eq!(load(db.conn()).unwrap(), Some(s));
    }

    #[test]
    fn normalize_keeps_active_tab() {
        let s = Session {
            tabs: vec![tab("https://a.com/", false), tab("", false), tab("https://b.com/", false), tab("https://p.com/", true)],
            active: 2,
            window: None,
        }
        .normalized(10);
        assert_eq!(s.tabs[0].url, "https://p.com/", "pinned first");
        assert_eq!(s.tabs[s.active].url, "https://b.com/");
        let empty = Session { tabs: vec![], active: 5, window: None }.normalized(10);
        assert_eq!(empty.active, 0);
    }
}
