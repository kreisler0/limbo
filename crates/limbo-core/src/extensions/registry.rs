//! Limbo's record of installed extensions (the WebView2 profile holds the
//! actual registration; this keeps the source for updates and toolbar state).

use rusqlite::{Connection, params};
use serde::Serialize;

use crate::error::Result;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InstalledExtension {
    pub id: String,
    pub name: String,
    pub version: String,
    pub path: String,
    /// chrome-web-store | edge-add-ons | file | unpacked
    pub source: String,
    pub installed_us: i64,
    pub updated_us: Option<i64>,
    /// Shown in the toolbar (max 3 visible, the rest in the overflow menu).
    pub pinned: bool,
    pub position: i64,
}

pub fn upsert(conn: &Connection, e: &InstalledExtension) -> Result<()> {
    conn.execute(
        "INSERT INTO extensions(id, name, version, path, source, installed_us, updated_us, pinned, position)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)
         ON CONFLICT(id) DO UPDATE SET name = excluded.name, version = excluded.version, path = excluded.path,
           source = excluded.source, updated_us = excluded.updated_us",
        params![e.id, e.name, e.version, e.path, e.source, e.installed_us, e.updated_us, e.pinned, e.position],
    )?;
    Ok(())
}

pub fn list(conn: &Connection) -> Result<Vec<InstalledExtension>> {
    let mut stmt = conn.prepare_cached(
        "SELECT id, name, version, path, source, installed_us, updated_us, pinned, position
         FROM extensions ORDER BY position, installed_us",
    )?;
    let rows = stmt
        .query_map([], |r| {
            Ok(InstalledExtension {
                id: r.get(0)?,
                name: r.get(1)?,
                version: r.get(2)?,
                path: r.get(3)?,
                source: r.get(4)?,
                installed_us: r.get(5)?,
                updated_us: r.get(6)?,
                pinned: r.get(7)?,
                position: r.get(8)?,
            })
        })?
        .collect::<Result<_, _>>()?;
    Ok(rows)
}

pub fn get(conn: &Connection, id: &str) -> Result<Option<InstalledExtension>> {
    Ok(list(conn)?.into_iter().find(|e| e.id == id))
}

pub fn remove(conn: &Connection, id: &str) -> Result<()> {
    conn.execute("DELETE FROM extensions WHERE id = ?1", [id])?;
    Ok(())
}

pub fn set_pinned(conn: &Connection, id: &str, pinned: bool) -> Result<()> {
    conn.execute("UPDATE extensions SET pinned = ?2 WHERE id = ?1", params![id, pinned])?;
    Ok(())
}

pub fn next_position(conn: &Connection) -> Result<i64> {
    Ok(conn.query_row("SELECT COALESCE(MAX(position), -1) + 1 FROM extensions", [], |r| r.get(0))?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::Db;

    #[test]
    fn roundtrip() {
        let db = Db::open_in_memory().unwrap();
        let c = db.conn();
        let mut e = InstalledExtension {
            id: "ddkjiahejlhfcafbddmgiahcphecmpfh".into(),
            name: "uBO Lite".into(),
            version: "1".into(),
            path: "C:\\x".into(),
            source: "chrome-web-store".into(),
            installed_us: 1,
            updated_us: None,
            pinned: true,
            position: next_position(c).unwrap(),
        };
        upsert(c, &e).unwrap();
        e.version = "2".into();
        upsert(c, &e).unwrap();
        set_pinned(c, &e.id, false).unwrap();
        let got = get(c, &e.id).unwrap().unwrap();
        assert_eq!((got.version.as_str(), got.pinned), ("2", false));
        assert_eq!(next_position(c).unwrap(), 1);
        remove(c, &e.id).unwrap();
        assert!(list(c).unwrap().is_empty());
    }
}
