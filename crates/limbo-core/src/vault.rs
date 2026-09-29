//! Password vault. Usernames and passwords are encrypted per value with a
//! [`SecretProtector`] (Windows DPAPI in the app); plaintext only lives in
//! [`Zeroizing`] buffers and is never logged.

use rusqlite::{Connection, OptionalExtension, params};
use serde::Serialize;
use zeroize::Zeroizing;

use crate::csv;
use crate::error::{Error, Result};
use crate::omnibox;

/// Encrypts secrets at rest. The Windows implementation uses DPAPI
/// (`CryptProtectData`, current-user scope, app-specific entropy).
pub trait SecretProtector: Send + Sync {
    fn protect(&self, plaintext: &[u8]) -> Result<Vec<u8>>;
    fn unprotect(&self, ciphertext: &[u8]) -> Result<Zeroizing<Vec<u8>>>;
}

/// Where a login came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum LoginSource {
    Native = 0,
    Firefox = 1,
    Csv = 2,
}

/// What the UI may see without re-authentication: everything but the password.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LoginSummary {
    pub id: i64,
    pub guid: String,
    pub origin: String,
    pub action_origin: Option<String>,
    pub realm: Option<String>,
    pub username: String,
    pub username_field: Option<String>,
    pub password_field: Option<String>,
    pub created_us: i64,
    pub last_used_us: Option<i64>,
    pub changed_us: Option<i64>,
    pub times_used: i64,
    /// Matched by registrable domain rather than exact origin (needs confirmation).
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub same_site_only: bool,
}

/// A login to store.
#[derive(Clone)]
pub struct NewLogin {
    pub origin: String,
    pub action_origin: Option<String>,
    pub realm: Option<String>,
    pub username: Zeroizing<String>,
    pub password: Zeroizing<String>,
    pub username_field: Option<String>,
    pub password_field: Option<String>,
    pub guid: Option<String>,
    pub created_us: Option<i64>,
    pub last_used_us: Option<i64>,
    pub changed_us: Option<i64>,
    pub times_used: i64,
}

impl std::fmt::Debug for NewLogin {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("NewLogin").field("origin", &self.origin).finish_non_exhaustive()
    }
}

impl NewLogin {
    pub fn new(origin: &str, username: &str, password: &str) -> NewLogin {
        NewLogin {
            origin: origin.to_string(),
            action_origin: None,
            realm: None,
            username: Zeroizing::new(username.to_string()),
            password: Zeroizing::new(password.to_string()),
            username_field: None,
            password_field: None,
            guid: None,
            created_us: None,
            last_used_us: None,
            changed_us: None,
            times_used: 0,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum SaveOutcome {
    Added { id: i64 },
    Updated { id: i64 },
    Unchanged { id: i64 },
}

/// What the "Save password?" popover should offer after a form submit.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum SavePrompt {
    /// Nothing to do (already saved, or the user said "never" for this site).
    None,
    Save,
    Update { id: i64 },
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportCounts {
    pub imported: usize,
    pub merged: usize,
    pub failed: usize,
}

fn decrypt_string(p: &dyn SecretProtector, blob: &[u8]) -> Result<Zeroizing<String>> {
    let bytes = p.unprotect(blob)?;
    let s = std::str::from_utf8(&bytes).map_err(|_| Error::Crypto("stored secret isn't UTF-8".into()))?;
    Ok(Zeroizing::new(s.to_string()))
}

/// Normalizes a URL or origin to `scheme://host[:port]`.
pub fn normalize_origin(s: &str) -> Option<String> {
    omnibox::origin_of(s).or_else(|| omnibox::origin_of(&format!("https://{s}")))
}

const COLUMNS: &str = "id, guid, origin, action_origin, realm, username_enc, username_field, password_field,
    created_us, last_used_us, changed_us, times_used";

fn summary(p: &dyn SecretProtector, r: &rusqlite::Row<'_>) -> Result<LoginSummary> {
    let enc: Vec<u8> = r.get(5)?;
    Ok(LoginSummary {
        id: r.get(0)?,
        guid: r.get(1)?,
        origin: r.get(2)?,
        action_origin: r.get(3)?,
        realm: r.get(4)?,
        username: decrypt_string(p, &enc)?.to_string(),
        username_field: r.get(6)?,
        password_field: r.get(7)?,
        created_us: r.get(8)?,
        last_used_us: r.get(9)?,
        changed_us: r.get(10)?,
        times_used: r.get(11)?,
        same_site_only: false,
    })
}

/// All logins (Settings -> Passwords), or those for one exact origin.
pub fn list(conn: &Connection, p: &dyn SecretProtector, origin: Option<&str>) -> Result<Vec<LoginSummary>> {
    let (sql, arg) = match origin {
        Some(o) => (format!("SELECT {COLUMNS} FROM logins WHERE origin = ?1 ORDER BY last_used_us DESC"), Some(o)),
        None => (format!("SELECT {COLUMNS} FROM logins ORDER BY origin, id"), None),
    };
    let mut stmt = conn.prepare(&sql)?;
    let mut rows = match arg {
        Some(o) => stmt.query([o])?,
        None => stmt.query([])?,
    };
    let mut out = Vec::new();
    while let Some(r) = rows.next()? {
        out.push(summary(p, r)?);
    }
    Ok(out)
}

/// Logins to offer on a page: exact origin first, then same registrable domain
/// (flagged `same_site_only`, the UI asks before filling those).
/// `http:` pages only match exact `http:` origins.
pub fn for_page(conn: &Connection, p: &dyn SecretProtector, page_url: &str) -> Result<Vec<LoginSummary>> {
    let Some(origin) = omnibox::origin_of(page_url) else { return Ok(Vec::new()) };
    let mut out = list(conn, p, Some(&origin))?;
    let url = url::Url::parse(page_url).map_err(|e| Error::Format(e.to_string()))?;
    if url.scheme() != "https" {
        return Ok(out);
    }
    let Some(site) = url.host_str().and_then(omnibox::registrable_domain) else { return Ok(out) };
    let mut stmt = conn.prepare(&format!(
        "SELECT {COLUMNS} FROM logins WHERE origin != ?1 AND (origin = ?2 OR origin LIKE ?3) ORDER BY last_used_us DESC"
    ))?;
    let mut rows = stmt.query(params![origin, format!("https://{site}"), format!("https://%.{site}")])?;
    while let Some(r) = rows.next()? {
        let mut s = summary(p, r)?;
        // LIKE could match a port suffix or unrelated host; re-check the site.
        let host_site = url::Url::parse(&s.origin)
            .ok()
            .and_then(|u| u.host_str().and_then(omnibox::registrable_domain));
        if host_site.as_deref() == Some(site.as_str()) {
            s.same_site_only = true;
            out.push(s);
        }
    }
    Ok(out)
}

/// Decrypts one password (only after Windows Hello for reveal/copy, or for an
/// explicit fill on the matching origin).
pub fn password(conn: &Connection, p: &dyn SecretProtector, id: i64) -> Result<Zeroizing<String>> {
    let enc: Vec<u8> = conn
        .query_row("SELECT password_enc FROM logins WHERE id = ?1", [id], |r| r.get(0))
        .optional()?
        .ok_or_else(|| Error::NotFound(format!("login {id}")))?;
    decrypt_string(p, &enc)
}

fn find_same(conn: &Connection, p: &dyn SecretProtector, origin: &str, username: &str) -> Result<Option<i64>> {
    for s in list(conn, p, Some(origin))? {
        if s.username == username {
            return Ok(Some(s.id));
        }
    }
    Ok(None)
}

/// Adds or updates a login. Logins are unique per (origin, username).
pub fn save(conn: &Connection, p: &dyn SecretProtector, login: &NewLogin, source: LoginSource, now_us: i64) -> Result<SaveOutcome> {
    let origin = normalize_origin(&login.origin).ok_or_else(|| Error::Format("invalid origin".into()))?;
    if let Some(id) = find_same(conn, p, &origin, &login.username)? {
        let current = password(conn, p, id)?;
        if *current == *login.password {
            return Ok(SaveOutcome::Unchanged { id });
        }
        let enc = p.protect(login.password.as_bytes())?;
        conn.execute(
            "UPDATE logins SET password_enc = ?2, changed_us = ?3 WHERE id = ?1",
            params![id, enc, login.changed_us.unwrap_or(now_us)],
        )?;
        return Ok(SaveOutcome::Updated { id });
    }
    let user_enc = p.protect(login.username.as_bytes())?;
    let pass_enc = p.protect(login.password.as_bytes())?;
    let guid = login.guid.clone().unwrap_or_else(|| format!("{{{}}}", uuid::Uuid::new_v4()));
    let action = login.action_origin.as_deref().and_then(normalize_origin);
    conn.execute(
        "INSERT INTO logins(guid, origin, action_origin, realm, username_enc, password_enc, username_field,
           password_field, created_us, last_used_us, changed_us, times_used, source)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)",
        params![
            guid,
            origin,
            action,
            login.realm,
            user_enc,
            pass_enc,
            login.username_field,
            login.password_field,
            login.created_us.unwrap_or(now_us),
            login.last_used_us,
            login.changed_us.or(login.created_us).unwrap_or(now_us),
            login.times_used,
            source as i64
        ],
    )?;
    Ok(SaveOutcome::Added { id: conn.last_insert_rowid() })
}

/// Decides what to offer after a login form was submitted.
pub fn save_prompt(conn: &Connection, p: &dyn SecretProtector, page_url: &str, username: &str, password_: &str) -> Result<SavePrompt> {
    let Some(origin) = omnibox::origin_of(page_url) else { return Ok(SavePrompt::None) };
    if password_.is_empty() || is_never_save(conn, &origin)? {
        return Ok(SavePrompt::None);
    }
    match find_same(conn, p, &origin, username)? {
        Some(id) => {
            if *password(conn, p, id)? == password_ { Ok(SavePrompt::None) } else { Ok(SavePrompt::Update { id }) }
        }
        None => Ok(SavePrompt::Save),
    }
}

pub fn touch_used(conn: &Connection, id: i64, now_us: i64) -> Result<()> {
    conn.execute("UPDATE logins SET last_used_us = ?2, times_used = times_used + 1 WHERE id = ?1", params![id, now_us])?;
    Ok(())
}

pub fn delete(conn: &Connection, id: i64) -> Result<()> {
    conn.execute("DELETE FROM logins WHERE id = ?1", [id])?;
    Ok(())
}

pub fn delete_all(conn: &Connection) -> Result<usize> {
    Ok(conn.execute("DELETE FROM logins", [])?)
}

pub fn never_save(conn: &Connection, origin: &str) -> Result<()> {
    conn.execute("INSERT OR IGNORE INTO never_save_logins(origin) VALUES (?1)", [origin])?;
    Ok(())
}

pub fn is_never_save(conn: &Connection, origin: &str) -> Result<bool> {
    Ok(conn.query_row("SELECT EXISTS(SELECT 1 FROM never_save_logins WHERE origin = ?1)", [origin], |r| r.get(0))?)
}

/// Bulk import; merges by (origin, username).
pub fn import(conn: &mut Connection, p: &dyn SecretProtector, logins: &[NewLogin], source: LoginSource, now_us: i64) -> Result<ImportCounts> {
    let tx = conn.transaction()?;
    let mut counts = ImportCounts::default();
    for l in logins {
        match save(&tx, p, l, source, now_us) {
            Ok(SaveOutcome::Added { .. }) => counts.imported += 1,
            Ok(SaveOutcome::Updated { .. } | SaveOutcome::Unchanged { .. }) => counts.merged += 1,
            Err(Error::Protect(e)) => return Err(Error::Protect(e)),
            Err(_) => counts.failed += 1,
        }
    }
    tx.commit()?;
    Ok(counts)
}

const CSV_HEADER: &[&str] = &[
    "name", "url", "username", "password", "httpRealm", "formActionOrigin", "guid", "timeCreated",
    "timeLastUsed", "timePasswordChanged",
];

/// CSV export readable by both Chrome (name,url,username,password) and
/// Firefox (url,username,password,httpRealm,formActionOrigin,guid,time*).
/// Times are milliseconds, as Firefox writes them.
pub fn export_csv(conn: &Connection, p: &dyn SecretProtector) -> Result<Zeroizing<String>> {
    let mut out = Zeroizing::new(csv::row(CSV_HEADER));
    for s in list(conn, p, None)? {
        let pw = password(conn, p, s.id)?;
        let host = url::Url::parse(&s.origin).ok().and_then(|u| u.host_str().map(str::to_string)).unwrap_or_default();
        let ms = |us: Option<i64>| us.map(|v| (v / 1000).to_string()).unwrap_or_default();
        let line = Zeroizing::new(csv::row(&[
            &host,
            &s.origin,
            &s.username,
            &pw,
            s.realm.as_deref().unwrap_or(""),
            s.action_origin.as_deref().unwrap_or(""),
            &s.guid,
            &ms(Some(s.created_us)),
            &ms(s.last_used_us),
            &ms(s.changed_us),
        ]));
        out.push_str(&line);
    }
    Ok(out)
}

/// Parses a Firefox (`about:logins` -> Export) or Chrome password CSV.
pub fn parse_csv(text: &str) -> Result<Vec<NewLogin>> {
    let rows = csv::parse(text);
    let Some((header, body)) = rows.split_first() else { return Ok(Vec::new()) };
    let col = |name: &str| header.iter().position(|h| h.trim().eq_ignore_ascii_case(name));
    let (Some(url_c), Some(user_c), Some(pass_c)) = (col("url"), col("username"), col("password")) else {
        return Err(Error::Format("CSV needs url, username and password columns".into()));
    };
    let (realm_c, action_c, guid_c) = (col("httpRealm"), col("formActionOrigin"), col("guid"));
    let (created_c, used_c, changed_c) = (col("timeCreated"), col("timeLastUsed"), col("timePasswordChanged"));
    let get = |row: &Vec<String>, c: Option<usize>| c.and_then(|i| row.get(i)).filter(|s| !s.is_empty()).cloned();
    let ms_to_us = |row: &Vec<String>, c: Option<usize>| get(row, c).and_then(|s| s.parse::<i64>().ok()).map(|ms| ms * 1000);
    let mut out = Vec::new();
    for row in body {
        let Some(url) = get(row, Some(url_c)) else { continue };
        let Some(origin) = normalize_origin(&url) else { continue };
        let mut l = NewLogin::new(
            &origin,
            row.get(user_c).map(String::as_str).unwrap_or(""),
            row.get(pass_c).map(String::as_str).unwrap_or(""),
        );
        if l.password.is_empty() {
            continue;
        }
        l.realm = get(row, realm_c);
        l.action_origin = get(row, action_c);
        l.guid = get(row, guid_c);
        l.created_us = ms_to_us(row, created_c);
        l.last_used_us = ms_to_us(row, used_c);
        l.changed_us = ms_to_us(row, changed_c);
        out.push(l);
    }
    Ok(out)
}

/// Test-only protector: reversible, obviously not secure.
#[cfg(any(test, feature = "test-protector"))]
pub struct TestProtector;

#[cfg(any(test, feature = "test-protector"))]
impl SecretProtector for TestProtector {
    fn protect(&self, plaintext: &[u8]) -> Result<Vec<u8>> {
        let mut v = b"TP1".to_vec();
        v.extend(plaintext.iter().map(|b| b ^ 0x5A));
        Ok(v)
    }
    fn unprotect(&self, ciphertext: &[u8]) -> Result<Zeroizing<Vec<u8>>> {
        let body = ciphertext.strip_prefix(b"TP1").ok_or_else(|| Error::Protect("bad blob".into()))?;
        Ok(Zeroizing::new(body.iter().map(|b| b ^ 0x5A).collect()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::Db;

    const P: &TestProtector = &TestProtector;

    #[test]
    fn save_merge_update() {
        let db = Db::open_in_memory().unwrap();
        let c = db.conn();
        let l = NewLogin::new("https://github.com/login", "octocat", "hunter2");
        assert!(matches!(save(c, P, &l, LoginSource::Native, 1).unwrap(), SaveOutcome::Added { .. }));
        assert!(matches!(save(c, P, &l, LoginSource::Native, 2).unwrap(), SaveOutcome::Unchanged { .. }));
        let l2 = NewLogin::new("https://github.com", "octocat", "correct horse");
        let SaveOutcome::Updated { id } = save(c, P, &l2, LoginSource::Native, 3).unwrap() else { panic!() };
        assert_eq!(*password(c, P, id).unwrap(), "correct horse");
        let all = list(c, P, None).unwrap();
        assert_eq!(all.len(), 1);
        assert_eq!(all[0].origin, "https://github.com");
        assert_eq!(all[0].username, "octocat");
        assert_eq!(all[0].changed_us, Some(3));

        // Secrets are not stored in plaintext.
        let raw: Vec<u8> = c.query_row("SELECT password_enc FROM logins", [], |r| r.get(0)).unwrap();
        assert!(!raw.windows(7).any(|w| w == b"correct"));
    }

    #[test]
    fn page_matching_and_prompts() {
        let db = Db::open_in_memory().unwrap();
        let c = db.conn();
        save(c, P, &NewLogin::new("https://accounts.google.com", "me@gmail.com", "pw1"), LoginSource::Native, 1).unwrap();
        save(c, P, &NewLogin::new("https://google.com", "other@gmail.com", "pw2"), LoginSource::Native, 1).unwrap();
        save(c, P, &NewLogin::new("https://evilgoogle.com", "x", "pw3"), LoginSource::Native, 1).unwrap();
        save(c, P, &NewLogin::new("http://intranet.local", "admin", "pw4"), LoginSource::Native, 1).unwrap();

        let m = for_page(c, P, "https://accounts.google.com/signin?x=1").unwrap();
        assert_eq!(m.len(), 2);
        assert!(!m[0].same_site_only);
        assert_eq!(m[1].origin, "https://google.com");
        assert!(m[1].same_site_only);

        assert_eq!(for_page(c, P, "http://intranet.local/login").unwrap().len(), 1);
        assert!(for_page(c, P, "data:text/html,hi").unwrap().is_empty());

        assert_eq!(save_prompt(c, P, "https://accounts.google.com/x", "me@gmail.com", "pw1").unwrap(), SavePrompt::None);
        assert!(matches!(save_prompt(c, P, "https://accounts.google.com/x", "me@gmail.com", "new").unwrap(), SavePrompt::Update { .. }));
        assert_eq!(save_prompt(c, P, "https://new.site/", "me", "pw").unwrap(), SavePrompt::Save);
        never_save(c, "https://new.site").unwrap();
        assert_eq!(save_prompt(c, P, "https://new.site/", "me", "pw").unwrap(), SavePrompt::None);
    }

    #[test]
    fn csv_roundtrip_and_import_counts() {
        let mut db = Db::open_in_memory().unwrap();
        save(db.conn(), P, &NewLogin::new("https://a.com", "u,1", "p\"1"), LoginSource::Native, 1_000).unwrap();
        save(db.conn(), P, &NewLogin::new("https://b.com", "u2", "p2"), LoginSource::Native, 2_000).unwrap();
        let text = export_csv(db.conn(), P).unwrap();
        assert!(text.starts_with("name,url,username,password,httpRealm"));
        let parsed = parse_csv(&text).unwrap();
        assert_eq!(parsed.len(), 2);
        assert_eq!(*parsed[0].username, "u,1");
        assert_eq!(*parsed[0].password, "p\"1");
        assert_eq!(parsed[0].created_us, Some(1_000));

        let mut db2 = Db::open_in_memory().unwrap();
        let counts = import(db2.conn_mut(), P, &parsed, LoginSource::Csv, 5).unwrap();
        assert_eq!(counts, ImportCounts { imported: 2, merged: 0, failed: 0 });
        let counts = import(db2.conn_mut(), P, &parsed, LoginSource::Csv, 5).unwrap();
        assert_eq!(counts, ImportCounts { imported: 0, merged: 2, failed: 0 });
        let _ = db.conn_mut();
    }

    #[test]
    fn chrome_csv_and_bad_rows() {
        let text = "name,url,username,password,note\r\ngithub.com,https://github.com/session,me,pw,\r\nbad,,x,y,\r\nempty,https://x.com,u,,\r\n";
        let parsed = parse_csv(text).unwrap();
        assert_eq!(parsed.len(), 1);
        assert_eq!(parsed[0].origin, "https://github.com");
        assert!(parse_csv("a,b\n1,2").is_err());
    }
}
