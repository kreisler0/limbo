//! Favicon cache: icon blobs keyed by icon URL, mapped to page URLs.

use rusqlite::{Connection, OptionalExtension, params};

use crate::error::Result;

/// Best-effort MIME type from magic bytes.
pub fn sniff_mime(data: &[u8]) -> &'static str {
    if data.starts_with(b"\x89PNG\r\n\x1a\n") {
        "image/png"
    } else if data.starts_with(&[0, 0, 1, 0]) {
        "image/x-icon"
    } else if data.starts_with(b"GIF8") {
        "image/gif"
    } else if data.starts_with(&[0xFF, 0xD8, 0xFF]) {
        "image/jpeg"
    } else if data.len() > 12 && &data[0..4] == b"RIFF" && &data[8..12] == b"WEBP" {
        "image/webp"
    } else if data.starts_with(b"BM") {
        "image/bmp"
    } else {
        let head = String::from_utf8_lossy(&data[..data.len().min(256)]).to_ascii_lowercase();
        if head.contains("<svg") { "image/svg+xml" } else { "application/octet-stream" }
    }
}

/// Stores (or replaces) an icon and links it to a page. Returns the icon id.
pub fn put(conn: &Connection, page_url: &str, icon_url: &str, data: &[u8], width: u32, now_us: i64) -> Result<i64> {
    let mime = sniff_mime(data);
    conn.execute(
        "INSERT INTO favicons(icon_url, data, mime, width, updated_us) VALUES (?1, ?2, ?3, ?4, ?5)
         ON CONFLICT(icon_url) DO UPDATE SET data = excluded.data, mime = excluded.mime,
           width = excluded.width, updated_us = excluded.updated_us",
        params![icon_url, data, mime, width, now_us],
    )?;
    let id: i64 = conn.query_row("SELECT id FROM favicons WHERE icon_url = ?1", [icon_url], |r| r.get(0))?;
    conn.execute(
        "INSERT INTO page_icons(page_url, favicon_id) VALUES (?1, ?2)
         ON CONFLICT(page_url) DO UPDATE SET favicon_id = excluded.favicon_id",
        params![page_url, id],
    )?;
    Ok(id)
}

/// Icon for a page: exact page first, then any page on the same host.
pub fn for_page(conn: &Connection, page_url: &str) -> Result<Option<(String, Vec<u8>)>> {
    let exact = conn
        .query_row(
            "SELECT f.mime, f.data FROM page_icons p JOIN favicons f ON f.id = p.favicon_id WHERE p.page_url = ?1",
            [page_url],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()?;
    if exact.is_some() {
        return Ok(exact);
    }
    let Ok(url) = url::Url::parse(page_url) else { return Ok(None) };
    let Some(host) = url.host_str() else { return Ok(None) };
    let prefix = format!("{}://{}/", url.scheme(), host);
    let hi = format!("{prefix}\u{10FFFF}");
    Ok(conn
        .query_row(
            "SELECT f.mime, f.data FROM page_icons p JOIN favicons f ON f.id = p.favicon_id
             WHERE p.page_url >= ?1 AND p.page_url < ?2 ORDER BY f.width DESC LIMIT 1",
            params![prefix, hi],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()?)
}

/// Drops icons no page refers to.
pub fn prune(conn: &Connection) -> Result<usize> {
    Ok(conn.execute("DELETE FROM favicons WHERE id NOT IN (SELECT favicon_id FROM page_icons)", [])?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::Db;

    const PNG: &[u8] = b"\x89PNG\r\n\x1a\n0000";

    #[test]
    fn sniffing() {
        assert_eq!(sniff_mime(PNG), "image/png");
        assert_eq!(sniff_mime(&[0, 0, 1, 0, 1]), "image/x-icon");
        assert_eq!(sniff_mime(b"<?xml version='1.0'?><svg xmlns=..."), "image/svg+xml");
        assert_eq!(sniff_mime(b"RIFF1234WEBPVP8 "), "image/webp");
        assert_eq!(sniff_mime(b"???"), "application/octet-stream");
    }

    #[test]
    fn put_lookup_and_host_fallback() {
        let db = Db::open_in_memory().unwrap();
        let c = db.conn();
        put(c, "https://github.com/rust-lang", "https://github.com/favicon.ico", PNG, 32, 1).unwrap();
        let (mime, data) = for_page(c, "https://github.com/rust-lang").unwrap().unwrap();
        assert_eq!(mime, "image/png");
        assert_eq!(data, PNG);
        assert!(for_page(c, "https://github.com/other/page").unwrap().is_some(), "same-host fallback");
        assert!(for_page(c, "https://gitlab.com/").unwrap().is_none());
        // Re-pointing the page leaves the old icon orphaned until pruned.
        put(c, "https://github.com/rust-lang", "https://github.com/new.png", PNG, 64, 2).unwrap();
        assert_eq!(prune(c).unwrap(), 1);
    }
}
