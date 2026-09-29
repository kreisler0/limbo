//! Cookies from `cookies.sqlite`. The host writes them into WebView2's cookie
//! manager. Google may reject imported session cookies (device-bound sessions),
//! so a one-time Google sign-in can still be needed.

use rusqlite::Connection;
use serde::Serialize;

use crate::error::Result;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum SameSite {
    None,
    Lax,
    Strict,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportedCookie {
    pub name: String,
    pub value: String,
    /// As Firefox stores it: a leading dot means a domain cookie, none means host-only.
    pub host: String,
    pub path: String,
    /// Seconds since the Unix epoch.
    pub expires: f64,
    pub secure: bool,
    pub http_only: bool,
    pub same_site: SameSite,
}

/// `moz_cookies.expiry` was seconds for years; newer Firefox versions store
/// milliseconds. Anything past year ~5000 in seconds must be milliseconds.
pub fn normalize_expiry(raw: i64) -> i64 {
    if raw > 100_000_000_000 { raw / 1000 } else { raw }
}

/// Reads unexpired, first-party cookies (no container/partition attributes).
pub fn read(conn: &Connection, now_s: i64) -> Result<Vec<ImportedCookie>> {
    let has_same_site = conn
        .prepare("SELECT name FROM pragma_table_info('moz_cookies') WHERE name = 'sameSite'")?
        .exists([])?;
    let sql = format!(
        "SELECT name, value, host, path, expiry, isSecure, isHttpOnly, {}, COALESCE(originAttributes, '')
         FROM moz_cookies",
        if has_same_site { "sameSite" } else { "0" }
    );
    let mut stmt = conn.prepare(&sql)?;
    let mut rows = stmt.query([])?;
    let mut out = Vec::new();
    while let Some(r) = rows.next()? {
        let origin_attributes: String = r.get(8)?;
        if !origin_attributes.is_empty() {
            continue;
        }
        let expires = normalize_expiry(r.get::<_, Option<i64>>(4)?.unwrap_or(0));
        if expires <= now_s {
            continue;
        }
        let host: String = r.get(2)?;
        if host.is_empty() {
            continue;
        }
        out.push(ImportedCookie {
            name: r.get(0)?,
            value: r.get(1)?,
            host,
            path: r.get::<_, Option<String>>(3)?.filter(|p| !p.is_empty()).unwrap_or_else(|| "/".into()),
            expires: expires as f64,
            secure: r.get::<_, Option<i64>>(5)?.unwrap_or(0) != 0,
            http_only: r.get::<_, Option<i64>>(6)?.unwrap_or(0) != 0,
            same_site: match r.get::<_, Option<i64>>(7)?.unwrap_or(0) {
                1 => SameSite::Lax,
                2 => SameSite::Strict,
                _ => SameSite::None,
            },
        });
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn expiry_units() {
        assert_eq!(normalize_expiry(1_900_000_000), 1_900_000_000);
        assert_eq!(normalize_expiry(1_900_000_000_000), 1_900_000_000);
        assert_eq!(normalize_expiry(0), 0);
    }
}
