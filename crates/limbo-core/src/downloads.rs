//! The persisted downloads list (the files themselves are WebView2's job).

use rusqlite::{Connection, params};
use serde::{Deserialize, Serialize};

use crate::error::Result;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum DownloadState {
    InProgress,
    Paused,
    Completed,
    Interrupted,
    Cancelled,
}

impl DownloadState {
    fn as_str(self) -> &'static str {
        match self {
            DownloadState::InProgress => "inProgress",
            DownloadState::Paused => "paused",
            DownloadState::Completed => "completed",
            DownloadState::Interrupted => "interrupted",
            DownloadState::Cancelled => "cancelled",
        }
    }

    fn parse(s: &str) -> DownloadState {
        match s {
            "paused" => DownloadState::Paused,
            "completed" => DownloadState::Completed,
            "cancelled" => DownloadState::Cancelled,
            "inProgress" => DownloadState::InProgress,
            _ => DownloadState::Interrupted,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DownloadRecord {
    pub id: i64,
    pub url: String,
    pub path: String,
    pub mime: Option<String>,
    pub total_bytes: Option<i64>,
    pub received_bytes: i64,
    pub state: DownloadState,
    /// Danger verdict reported by the engine (e.g. "dangerous", "uncommon").
    pub danger: Option<String>,
    pub started_us: i64,
    pub ended_us: Option<i64>,
}

/// Records a new download. `id` lets the host use its own ids (`None` = auto).
pub fn insert(
    conn: &Connection,
    id: Option<i64>,
    url: &str,
    path: &str,
    mime: Option<&str>,
    total: Option<i64>,
    now_us: i64,
) -> Result<i64> {
    conn.execute(
        "INSERT OR REPLACE INTO downloads(id, url, path, mime, total_bytes, received_bytes, state, started_us)
         VALUES (?1, ?2, ?3, ?4, ?5, 0, 'inProgress', ?6)",
        params![id, url, path, mime, total, now_us],
    )?;
    Ok(conn.last_insert_rowid())
}

/// Highest id in use (the host continues from here).
pub fn max_id(conn: &Connection) -> Result<i64> {
    Ok(conn.query_row("SELECT COALESCE(MAX(id), 0) FROM downloads", [], |r| r.get(0))?)
}

pub fn update(
    conn: &Connection,
    id: i64,
    received: i64,
    total: Option<i64>,
    state: DownloadState,
    danger: Option<&str>,
    now_us: i64,
) -> Result<()> {
    let ended = matches!(state, DownloadState::Completed | DownloadState::Cancelled | DownloadState::Interrupted)
        .then_some(now_us);
    conn.execute(
        "UPDATE downloads SET received_bytes = ?2, total_bytes = COALESCE(?3, total_bytes), state = ?4,
           danger = COALESCE(?5, danger), ended_us = COALESCE(?6, ended_us)
         WHERE id = ?1",
        params![id, received, total, state.as_str(), danger, ended],
    )?;
    Ok(())
}

pub fn set_path(conn: &Connection, id: i64, path: &str) -> Result<()> {
    conn.execute("UPDATE downloads SET path = ?2 WHERE id = ?1", params![id, path])?;
    Ok(())
}

pub fn list(conn: &Connection, limit: usize) -> Result<Vec<DownloadRecord>> {
    let mut stmt = conn.prepare_cached(
        "SELECT id, url, path, mime, total_bytes, received_bytes, state, danger, started_us, ended_us
         FROM downloads ORDER BY started_us DESC, id DESC LIMIT ?1",
    )?;
    let rows = stmt
        .query_map([limit as i64], |r| {
            Ok(DownloadRecord {
                id: r.get(0)?,
                url: r.get(1)?,
                path: r.get(2)?,
                mime: r.get(3)?,
                total_bytes: r.get(4)?,
                received_bytes: r.get(5)?,
                state: DownloadState::parse(&r.get::<_, String>(6)?),
                danger: r.get(7)?,
                started_us: r.get(8)?,
                ended_us: r.get(9)?,
            })
        })?
        .collect::<Result<_, _>>()?;
    Ok(rows)
}

/// Downloads still marked in progress when the app starts were interrupted.
pub fn mark_stale_interrupted(conn: &Connection) -> Result<usize> {
    Ok(conn.execute("UPDATE downloads SET state = 'interrupted' WHERE state IN ('inProgress', 'paused')", [])?)
}

pub fn remove(conn: &Connection, id: i64) -> Result<()> {
    conn.execute("DELETE FROM downloads WHERE id = ?1", [id])?;
    Ok(())
}

pub fn clear_finished(conn: &Connection) -> Result<usize> {
    Ok(conn.execute("DELETE FROM downloads WHERE state NOT IN ('inProgress', 'paused')", [])?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::Db;

    #[test]
    fn lifecycle() {
        let db = Db::open_in_memory().unwrap();
        let c = db.conn();
        let id =
            insert(c, None, "https://x.com/a.zip", "C:\\Users\\me\\Downloads\\a.zip", Some("application/zip"), None, 1)
                .unwrap();
        update(c, id, 50, Some(100), DownloadState::InProgress, None, 2).unwrap();
        let d = &list(c, 10).unwrap()[0];
        assert_eq!((d.received_bytes, d.total_bytes, d.state), (50, Some(100), DownloadState::InProgress));
        assert_eq!(d.ended_us, None);
        update(c, id, 100, None, DownloadState::Completed, None, 3).unwrap();
        let d = &list(c, 10).unwrap()[0];
        assert_eq!((d.state, d.ended_us, d.total_bytes), (DownloadState::Completed, Some(3), Some(100)));
        let id2 = insert(c, Some(42), "https://x.com/b.exe", "b.exe", None, None, 4).unwrap();
        assert_eq!(id2, 42);
        assert_eq!(max_id(c).unwrap(), 42);
        assert_eq!(mark_stale_interrupted(c).unwrap(), 1);
        assert_eq!(list(c, 10).unwrap()[0].id, id2);
        assert_eq!(clear_finished(c).unwrap(), 2);
    }
}
