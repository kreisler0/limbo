//! The single SQLite database (`browser.db`, WAL mode) and its schema.

use std::path::Path;

use rusqlite::{Connection, OpenFlags};

use crate::error::Result;

/// Current time in microseconds since the Unix epoch (Firefox's PRTime unit).
pub fn now_us() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_micros() as i64)
        .unwrap_or(0)
}

/// Firefox-compatible bookmark root GUIDs.
pub mod roots {
    pub const ROOT: &str = "root________";
    pub const MENU: &str = "menu________";
    pub const TOOLBAR: &str = "toolbar_____";
    /// "Other bookmarks" (Firefox calls it unfiled).
    pub const OTHER: &str = "unfiled_____";
    pub const MOBILE: &str = "mobile______";
}

const SCHEMA_V1: &str = r#"
CREATE TABLE places(
  id INTEGER PRIMARY KEY,
  url TEXT UNIQUE NOT NULL,
  title TEXT,
  host TEXT NOT NULL DEFAULT '',
  visit_count INTEGER NOT NULL DEFAULT 0,
  typed_count INTEGER NOT NULL DEFAULT 0,
  last_visit_us INTEGER,
  frecency INTEGER NOT NULL DEFAULT 0
);
CREATE INDEX places_frecency ON places(frecency DESC);
CREATE INDEX places_host ON places(host);
CREATE INDEX places_last_visit ON places(last_visit_us);

CREATE TABLE visits(
  id INTEGER PRIMARY KEY,
  place_id INTEGER NOT NULL REFERENCES places(id) ON DELETE CASCADE,
  visit_us INTEGER NOT NULL,
  transition INTEGER NOT NULL,
  source INTEGER NOT NULL DEFAULT 0 -- 0 native, 1 firefox
);
CREATE UNIQUE INDEX visits_place_time ON visits(place_id, visit_us);
CREATE INDEX visits_time ON visits(visit_us);

CREATE VIRTUAL TABLE places_fts USING fts5(
  url, title,
  content='places', content_rowid='id',
  prefix='2 3',
  tokenize="unicode61 remove_diacritics 2"
);
CREATE TRIGGER places_ai AFTER INSERT ON places BEGIN
  INSERT INTO places_fts(rowid, url, title) VALUES (new.id, new.url, new.title);
END;
CREATE TRIGGER places_ad AFTER DELETE ON places BEGIN
  INSERT INTO places_fts(places_fts, rowid, url, title) VALUES ('delete', old.id, old.url, old.title);
END;
CREATE TRIGGER places_au AFTER UPDATE OF url, title ON places BEGIN
  INSERT INTO places_fts(places_fts, rowid, url, title) VALUES ('delete', old.id, old.url, old.title);
  INSERT INTO places_fts(rowid, url, title) VALUES (new.id, new.url, new.title);
END;

-- Origin-level rows for inline autocomplete (like Firefox's moz_origins).
CREATE TABLE origins(
  host TEXT PRIMARY KEY,
  prefix TEXT NOT NULL,
  frecency INTEGER NOT NULL DEFAULT 0
) WITHOUT ROWID;
CREATE INDEX origins_frecency ON origins(frecency DESC);

CREATE TABLE bookmarks(
  id INTEGER PRIMARY KEY,
  parent_id INTEGER REFERENCES bookmarks(id) ON DELETE CASCADE,
  kind INTEGER NOT NULL, -- 0 url, 1 folder, 2 separator
  title TEXT NOT NULL DEFAULT '',
  url TEXT,
  position INTEGER NOT NULL DEFAULT 0,
  added_us INTEGER NOT NULL,
  modified_us INTEGER NOT NULL,
  guid TEXT UNIQUE NOT NULL
);
CREATE INDEX bookmarks_parent ON bookmarks(parent_id, position);
CREATE INDEX bookmarks_url ON bookmarks(url);

CREATE TABLE favicons(
  id INTEGER PRIMARY KEY,
  icon_url TEXT UNIQUE NOT NULL,
  data BLOB NOT NULL,
  mime TEXT NOT NULL,
  width INTEGER NOT NULL DEFAULT 0,
  updated_us INTEGER NOT NULL
);
CREATE TABLE page_icons(
  page_url TEXT PRIMARY KEY,
  favicon_id INTEGER NOT NULL REFERENCES favicons(id) ON DELETE CASCADE
) WITHOUT ROWID;
CREATE INDEX page_icons_favicon ON page_icons(favicon_id);

CREATE TABLE logins(
  id INTEGER PRIMARY KEY,
  guid TEXT UNIQUE NOT NULL,
  origin TEXT NOT NULL,
  action_origin TEXT,
  realm TEXT,
  username_enc BLOB NOT NULL,
  password_enc BLOB NOT NULL,
  username_field TEXT,
  password_field TEXT,
  created_us INTEGER NOT NULL,
  last_used_us INTEGER,
  changed_us INTEGER,
  times_used INTEGER NOT NULL DEFAULT 0,
  source INTEGER NOT NULL DEFAULT 0 -- 0 native, 1 firefox, 2 csv
);
CREATE INDEX logins_origin ON logins(origin);
CREATE TABLE never_save_logins(origin TEXT PRIMARY KEY) WITHOUT ROWID;

CREATE TABLE form_history(
  id INTEGER PRIMARY KEY,
  field_name TEXT NOT NULL,
  value TEXT NOT NULL,
  times_used INTEGER NOT NULL DEFAULT 1,
  first_used_us INTEGER,
  last_used_us INTEGER,
  UNIQUE(field_name, value)
);

CREATE TABLE site_permissions(
  origin TEXT NOT NULL,
  kind TEXT NOT NULL,
  allow INTEGER NOT NULL,
  updated_us INTEGER NOT NULL,
  PRIMARY KEY(origin, kind)
) WITHOUT ROWID;

CREATE TABLE site_zoom(host TEXT PRIMARY KEY, factor REAL NOT NULL) WITHOUT ROWID;

CREATE TABLE settings(key TEXT PRIMARY KEY, value TEXT NOT NULL) WITHOUT ROWID;

CREATE TABLE session(id INTEGER PRIMARY KEY CHECK (id = 1), data TEXT NOT NULL, saved_us INTEGER NOT NULL);

CREATE TABLE downloads(
  id INTEGER PRIMARY KEY,
  url TEXT NOT NULL,
  path TEXT NOT NULL,
  mime TEXT,
  total_bytes INTEGER,
  received_bytes INTEGER NOT NULL DEFAULT 0,
  state TEXT NOT NULL,
  danger TEXT,
  started_us INTEGER NOT NULL,
  ended_us INTEGER
);

CREATE TABLE extensions(
  id TEXT PRIMARY KEY,
  name TEXT NOT NULL,
  version TEXT NOT NULL,
  path TEXT NOT NULL,
  source TEXT NOT NULL, -- chrome-web-store | edge-add-ons | file | unpacked
  installed_us INTEGER NOT NULL,
  updated_us INTEGER,
  pinned INTEGER NOT NULL DEFAULT 1,
  position INTEGER NOT NULL DEFAULT 0
) WITHOUT ROWID;
"#;

const MIGRATIONS: &[&str] = &[SCHEMA_V1];

pub struct Db {
    conn: Connection,
}

impl Db {
    pub fn open(path: &Path) -> Result<Db> {
        let conn = Connection::open_with_flags(
            path,
            OpenFlags::SQLITE_OPEN_READ_WRITE | OpenFlags::SQLITE_OPEN_CREATE | OpenFlags::SQLITE_OPEN_NO_MUTEX,
        )?;
        Self::init(conn)
    }

    pub fn open_in_memory() -> Result<Db> {
        Self::init(Connection::open_in_memory()?)
    }

    fn init(conn: Connection) -> Result<Db> {
        // WAL + NORMAL sync: durable enough for a browser, far fewer fsyncs.
        // A small page cache (1 MB) keeps the host's footprint low.
        conn.execute_batch(
            "PRAGMA journal_mode=WAL;
             PRAGMA synchronous=NORMAL;
             PRAGMA foreign_keys=ON;
             PRAGMA cache_size=-1024;
             PRAGMA temp_store=MEMORY;
             PRAGMA busy_timeout=2000;",
        )?;
        let mut db = Db { conn };
        db.migrate()?;
        Ok(db)
    }

    fn migrate(&mut self) -> Result<()> {
        let version: i64 = self.conn.query_row("PRAGMA user_version", [], |r| r.get(0))?;
        for (i, sql) in MIGRATIONS.iter().enumerate().skip(version as usize) {
            let tx = self.conn.transaction()?;
            tx.execute_batch(sql)?;
            if i == 0 {
                crate::bookmarks::create_roots(&tx)?;
            }
            tx.pragma_update(None, "user_version", (i + 1) as i64)?;
            tx.commit()?;
        }
        Ok(())
    }

    pub fn conn(&self) -> &Connection {
        &self.conn
    }

    pub fn conn_mut(&mut self) -> &mut Connection {
        &mut self.conn
    }

    /// Releases SQLite's cached pages (called when the app goes idle or under
    /// memory pressure).
    pub fn shrink_memory(&self) {
        let _ = self.conn.execute_batch("PRAGMA shrink_memory;");
    }

    // --- settings -----------------------------------------------------------

    pub fn load_settings(&self) -> Result<crate::settings::Settings> {
        let mut stmt = self.conn.prepare("SELECT key, value FROM settings")?;
        let rows: Vec<(String, String)> =
            stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?.collect::<Result<_, _>>()?;
        Ok(crate::settings::Settings::from_rows(rows.iter().map(|(k, v)| (k.as_str(), v.as_str()))))
    }

    pub fn save_settings(&mut self, settings: &crate::settings::Settings) -> Result<()> {
        let tx = self.conn.transaction()?;
        for (k, v) in settings.to_rows() {
            tx.execute(
                "INSERT INTO settings(key, value) VALUES (?1, ?2)
                 ON CONFLICT(key) DO UPDATE SET value = excluded.value",
                (k, v),
            )?;
        }
        tx.commit()?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn opens_and_migrates_idempotently() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("browser.db");
        {
            let db = Db::open(&path).unwrap();
            let v: i64 = db.conn().query_row("PRAGMA user_version", [], |r| r.get(0)).unwrap();
            assert_eq!(v, MIGRATIONS.len() as i64);
            let mode: String = db.conn().query_row("PRAGMA journal_mode", [], |r| r.get(0)).unwrap();
            assert_eq!(mode, "wal");
        }
        let db = Db::open(&path).unwrap();
        let roots: i64 = db.conn().query_row("SELECT COUNT(*) FROM bookmarks", [], |r| r.get(0)).unwrap();
        assert_eq!(roots, 5, "roots created once");
    }

    #[test]
    fn settings_roundtrip() {
        let mut db = Db::open_in_memory().unwrap();
        let mut s = db.load_settings().unwrap();
        assert_eq!(s, crate::settings::Settings::default());
        s.developer_mode = true;
        s.theme = crate::settings::Theme::Dark;
        db.save_settings(&s).unwrap();
        assert_eq!(db.load_settings().unwrap(), s);
    }

    #[test]
    fn fts5_is_available() {
        let db = Db::open_in_memory().unwrap();
        db.conn().execute("INSERT INTO places(url, title) VALUES ('https://a.com/', 'Hello World')", []).unwrap();
        let n: i64 = db
            .conn()
            .query_row("SELECT COUNT(*) FROM places_fts WHERE places_fts MATCH '\"wor\"*'", [], |r| r.get(0))
            .unwrap();
        assert_eq!(n, 1);
    }
}
