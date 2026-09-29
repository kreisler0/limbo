//! Per-site state: remembered permissions and zoom levels.

use rusqlite::{Connection, OptionalExtension, params};
use serde::Serialize;

use crate::error::Result;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SitePermission {
    pub origin: String,
    /// camera, microphone, geolocation, notifications, clipboardRead, midi, multipleDownloads, ...
    pub kind: String,
    pub allow: bool,
    pub updated_us: i64,
}

pub fn permission(conn: &Connection, origin: &str, kind: &str) -> Result<Option<bool>> {
    Ok(conn
        .query_row("SELECT allow FROM site_permissions WHERE origin = ?1 AND kind = ?2", params![origin, kind], |r| {
            r.get(0)
        })
        .optional()?)
}

pub fn set_permission(conn: &Connection, origin: &str, kind: &str, allow: bool, now_us: i64) -> Result<()> {
    conn.execute(
        "INSERT INTO site_permissions(origin, kind, allow, updated_us) VALUES (?1, ?2, ?3, ?4)
         ON CONFLICT(origin, kind) DO UPDATE SET allow = excluded.allow, updated_us = excluded.updated_us",
        params![origin, kind, allow, now_us],
    )?;
    Ok(())
}

pub fn revoke_permission(conn: &Connection, origin: &str, kind: &str) -> Result<()> {
    conn.execute("DELETE FROM site_permissions WHERE origin = ?1 AND kind = ?2", params![origin, kind])?;
    Ok(())
}

/// Everything remembered for one origin (the site-info popover), or for all
/// origins when `origin` is `None` (Settings).
pub fn permissions(conn: &Connection, origin: Option<&str>) -> Result<Vec<SitePermission>> {
    let map = |r: &rusqlite::Row<'_>| {
        Ok(SitePermission { origin: r.get(0)?, kind: r.get(1)?, allow: r.get(2)?, updated_us: r.get(3)? })
    };
    let rows = match origin {
        Some(o) => {
            let mut stmt = conn.prepare_cached(
                "SELECT origin, kind, allow, updated_us FROM site_permissions WHERE origin = ?1 ORDER BY kind",
            )?;
            stmt.query_map([o], map)?.collect::<Result<_, _>>()?
        }
        None => {
            let mut stmt = conn
                .prepare_cached("SELECT origin, kind, allow, updated_us FROM site_permissions ORDER BY origin, kind")?;
            stmt.query_map([], map)?.collect::<Result<_, _>>()?
        }
    };
    Ok(rows)
}

pub const ZOOM_LEVELS: &[f64] =
    &[0.25, 0.33, 0.5, 0.67, 0.75, 0.8, 0.9, 1.0, 1.1, 1.25, 1.5, 1.75, 2.0, 2.5, 3.0, 4.0, 5.0];

/// Next zoom step (Ctrl +/-), like Chromium's preset levels.
pub fn step_zoom(current: f64, up: bool) -> f64 {
    let eps = 0.001;
    if up {
        ZOOM_LEVELS.iter().copied().find(|&z| z > current + eps).unwrap_or(*ZOOM_LEVELS.last().unwrap())
    } else {
        ZOOM_LEVELS.iter().rev().copied().find(|&z| z < current - eps).unwrap_or(ZOOM_LEVELS[0])
    }
}

pub fn zoom(conn: &Connection, host: &str) -> Result<Option<f64>> {
    Ok(conn.query_row("SELECT factor FROM site_zoom WHERE host = ?1", [host], |r| r.get(0)).optional()?)
}

/// Stores a site's zoom; storing the default removes the row.
pub fn set_zoom(conn: &Connection, host: &str, factor: f64, default: f64) -> Result<()> {
    if (factor - default).abs() < 0.001 {
        conn.execute("DELETE FROM site_zoom WHERE host = ?1", [host])?;
    } else {
        conn.execute(
            "INSERT INTO site_zoom(host, factor) VALUES (?1, ?2) ON CONFLICT(host) DO UPDATE SET factor = excluded.factor",
            params![host, factor],
        )?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::Db;

    #[test]
    fn permissions_roundtrip() {
        let db = Db::open_in_memory().unwrap();
        let c = db.conn();
        assert_eq!(permission(c, "https://meet.google.com", "camera").unwrap(), None);
        set_permission(c, "https://meet.google.com", "camera", true, 1).unwrap();
        set_permission(c, "https://meet.google.com", "microphone", false, 1).unwrap();
        set_permission(c, "https://meet.google.com", "camera", false, 2).unwrap();
        assert_eq!(permission(c, "https://meet.google.com", "camera").unwrap(), Some(false));
        assert_eq!(permissions(c, Some("https://meet.google.com")).unwrap().len(), 2);
        revoke_permission(c, "https://meet.google.com", "camera").unwrap();
        assert_eq!(permissions(c, None).unwrap().len(), 1);
    }

    #[test]
    fn zoom_steps_and_storage() {
        assert_eq!(step_zoom(1.0, true), 1.1);
        assert_eq!(step_zoom(1.0, false), 0.9);
        assert_eq!(step_zoom(5.0, true), 5.0);
        assert_eq!(step_zoom(0.25, false), 0.25);
        assert_eq!(step_zoom(1.05, true), 1.1);
        let db = Db::open_in_memory().unwrap();
        let c = db.conn();
        set_zoom(c, "example.com", 1.25, 1.0).unwrap();
        assert_eq!(zoom(c, "example.com").unwrap(), Some(1.25));
        set_zoom(c, "example.com", 1.0, 1.0).unwrap();
        assert_eq!(zoom(c, "example.com").unwrap(), None);
    }
}
