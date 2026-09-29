//! "Bring your stuff from Firefox": profile discovery and per-type importers.
//! Nothing here ever writes to the Firefox profile (databases are copied first).
//! Every importer is idempotent, so re-running an import merges.

pub mod addons;
pub mod cookies;
pub mod der;
pub mod favicons;
pub mod formhistory;
pub mod key4;
pub mod logins;
pub mod places;
pub mod profiles;
pub mod session;
pub mod snapshot;

#[cfg(test)]
mod fixture;

use std::path::Path;

use rusqlite::{Connection, OptionalExtension};
use serde::{Deserialize, Serialize};

use crate::error::{Error, Result};
use crate::vault::{self, ImportCounts, LoginSource, SecretProtector};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum DataType {
    History,
    Bookmarks,
    Favicons,
    Passwords,
    Cookies,
    FormHistory,
    Tabs,
    Addons,
}

impl DataType {
    pub const ALL: [DataType; 8] = [
        DataType::History,
        DataType::Bookmarks,
        DataType::Favicons,
        DataType::Passwords,
        DataType::Cookies,
        DataType::FormHistory,
        DataType::Tabs,
        DataType::Addons,
    ];
}

/// Listed in the summary as "not imported".
pub const NOT_IMPORTED: &[&str] =
    &["Site permissions and settings", "Downloads list", "Certificates", "Search engines (Limbo always uses Google)"];

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HistoryCounts {
    pub pages: usize,
    pub visits: usize,
}

/// Imports history in batches, reporting `(done, total)` pages.
pub fn import_history(
    db: &mut Connection,
    profile: &Path,
    mut progress: impl FnMut(usize, usize),
) -> Result<HistoryCounts> {
    let snap = snapshot::open(profile, "places.sqlite")?;
    let now = crate::db::now_us();
    let mut counts = HistoryCounts::default();
    places::read_history(&snap.conn, 5000, |batch, done, total| {
        let (p, v) = crate::history::import_places(db, &batch, 1, now)?;
        counts.pages += p;
        counts.visits += v;
        progress(done, total);
        Ok(())
    })?;
    Ok(counts)
}

pub fn import_bookmarks(db: &mut Connection, profile: &Path) -> Result<usize> {
    let snap = snapshot::open(profile, "places.sqlite")?;
    let items = places::read_bookmarks(&snap.conn)?;
    crate::bookmarks::import_items(db, &items)
}

/// Imports icons for pages Limbo knows about (run after history/bookmarks).
pub fn import_favicons(db: &mut Connection, profile: &Path) -> Result<usize> {
    let snap = match snapshot::open(profile, "favicons.sqlite") {
        Ok(s) => s,
        Err(Error::NotFound(_)) => return Ok(0),
        Err(e) => return Err(e),
    };
    let icons = {
        let mut known = db.prepare("SELECT 1 FROM places WHERE url = ?1")?;
        favicons::read(&snap.conn, |url| known.query_row([url], |_| Ok(())).optional().ok().flatten().is_some())?
    };
    let tx = db.transaction()?;
    let now = crate::db::now_us();
    for icon in &icons {
        crate::favicons::put(&tx, &icon.page_url, &icon.icon_url, &icon.data, icon.width, now)?;
    }
    tx.commit()?;
    Ok(icons.len())
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PasswordCounts {
    #[serde(flatten)]
    pub counts: ImportCounts,
    pub skipped: usize,
}

/// Decrypts Firefox logins and re-encrypts them into the vault immediately.
/// `Err(WrongPrimaryPassword)`: ask the user and call again.
pub fn import_passwords(
    db: &mut Connection,
    protector: &dyn SecretProtector,
    profile: &Path,
    primary_password: &str,
) -> Result<PasswordCounts> {
    let decrypted = logins::read(profile, primary_password)?;
    let mut counts = vault::import(db, protector, &decrypted.logins, LoginSource::Firefox, crate::db::now_us())?;
    counts.failed += decrypted.failed;
    Ok(PasswordCounts { counts, skipped: decrypted.skipped })
}

/// Cookies to hand to WebView2's cookie manager.
pub fn read_cookies(profile: &Path) -> Result<Vec<cookies::ImportedCookie>> {
    let snap = snapshot::open(profile, "cookies.sqlite")?;
    let now_s = crate::db::now_us() / 1_000_000;
    cookies::read(&snap.conn, now_s)
}

pub fn import_form_history(db: &mut Connection, profile: &Path) -> Result<usize> {
    let snap = match snapshot::open(profile, "formhistory.sqlite") {
        Ok(s) => s,
        Err(Error::NotFound(_)) => return Ok(0),
        Err(e) => return Err(e),
    };
    let entries = formhistory::read(&snap.conn)?;
    formhistory::store(db, &entries)
}

pub fn read_tabs(profile: &Path) -> Result<Vec<session::FirefoxTab>> {
    session::read(profile)
}

pub fn read_addons(profile: &Path) -> Result<Vec<addons::FirefoxAddon>> {
    addons::read(profile)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::{Db, roots};
    use crate::vault::TestProtector;

    #[test]
    fn full_import_from_synthetic_profile() {
        let dir = tempfile::tempdir().unwrap();
        let profile = dir.path();
        fixture::build(profile, "test-primary");

        let mut db = Db::open_in_memory().unwrap();
        let mut last = (0, 0);
        let h = import_history(db.conn_mut(), profile, |d, t| last = (d, t)).unwrap();
        assert_eq!(h, HistoryCounts { pages: fixture::HISTORY_PAGES, visits: fixture::HISTORY_VISITS });
        assert_eq!(last.0, fixture::HISTORY_PAGES);
        let b = import_bookmarks(db.conn_mut(), profile).unwrap();
        assert_eq!(b, fixture::BOOKMARKS);
        let f = import_favicons(db.conn_mut(), profile).unwrap();
        assert_eq!(f, 2);

        let p = &TestProtector;
        assert!(matches!(import_passwords(db.conn_mut(), p, profile, ""), Err(Error::WrongPrimaryPassword)));
        let pw = import_passwords(db.conn_mut(), p, profile, "test-primary").unwrap();
        assert_eq!(pw.counts.imported, fixture::LOGINS);
        assert_eq!(pw.counts.failed, 1, "the corrupted entry");
        assert_eq!(pw.skipped, 1, "Firefox Accounts entry");

        let c = read_cookies(profile).unwrap();
        assert_eq!(c.len(), 2, "expired and container cookies are skipped");
        let fh = import_form_history(db.conn_mut(), profile).unwrap();
        assert_eq!(fh, 2);
        let tabs = read_tabs(profile).unwrap();
        assert_eq!(tabs.len(), 3);
        let addons = read_addons(profile).unwrap();
        assert_eq!(addons.len(), 2);

        // Structure and content.
        let conn = db.conn();
        let toolbar = crate::bookmarks::tree(conn, crate::bookmarks::root_id(conn, roots::TOOLBAR).unwrap()).unwrap();
        assert_eq!(toolbar.children[0].title, "Dev");
        assert_eq!(toolbar.children[0].children[0].title, "Rust");
        assert_eq!(toolbar.children[1].title, "MDN");
        let logins = vault::list(conn, p, None).unwrap();
        let gh = logins.iter().find(|l| l.origin == "https://github.com").unwrap();
        assert_eq!(gh.username, "octocat");
        assert_eq!(&*vault::password(conn, p, gh.id).unwrap(), "hunter2");
        assert!(crate::favicons::for_page(conn, "https://www.rust-lang.org/").unwrap().is_some());

        // Re-running merges instead of duplicating.
        let h2 = import_history(db.conn_mut(), profile, |_, _| {}).unwrap();
        assert_eq!(h2, HistoryCounts::default());
        assert_eq!(import_bookmarks(db.conn_mut(), profile).unwrap(), 0);
        let pw2 = import_passwords(db.conn_mut(), p, profile, "test-primary").unwrap();
        assert_eq!(pw2.counts.imported, 0);
        assert_eq!(pw2.counts.merged, fixture::LOGINS);
    }

    /// Validates decryption against real Firefox-generated profiles from the
    /// firefox_decrypt project (not vendored; see tools/fetch-firefox-testdata.sh).
    #[test]
    fn real_firefox_profiles() {
        let Some(root) = std::env::var_os("LIMBO_FIREFOX_TESTDATA") else {
            eprintln!("LIMBO_FIREFOX_TESTDATA not set; skipping real-profile test");
            return;
        };
        let root = std::path::PathBuf::from(root);
        let primary = std::fs::read_to_string(root.join("master_password")).unwrap();
        let primary = primary.trim_end_matches(['\r', '\n']);
        let expected: Vec<(String, String)> = vec![
            ("doesntexist".into(), "xrbSDzYf94gfk".into()),
            ("onemore".into(), "}]\u{a2}\u{f6}\u{f0}\u{e6}[{".into()),
            (
                "c\u{f6}mplex".into(),
                "\u{441}\u{42e}\u{41b}\u{41e}\u{430}\u{436}\u{441}$4vz*V\u{e7}\u{e0}hxpfCbmwo".into(),
            ),
            ("j\u{e3}m\u{ef}e".into(), "Apassword\twithtabs,;colonandsemi'\"andquotes".into()),
        ];
        for name in
            ["test_profile_firefox_59", "test_profile_firefox_144", "test_profile_firefox_L\u{42e}\u{448}\u{440}"]
        {
            let profile = root.join(name);
            assert!(matches!(logins::read(&profile, "wrong"), Err(Error::WrongPrimaryPassword)), "{name}");
            let got = logins::read(&profile, primary).unwrap_or_else(|e| panic!("{name}: {e}"));
            let mut pairs: Vec<(String, String)> =
                got.logins.iter().map(|l| (l.username.to_string(), l.password.to_string())).collect();
            pairs.sort();
            let mut want = expected.clone();
            want.sort();
            assert_eq!(pairs, want, "{name}");
            assert_eq!(got.failed, 0, "{name}");
            assert!(got.logins.iter().all(|l| l.origin == "https://github.com"));
        }
        let nopass = logins::read(&root.join("test_profile_firefox_nopassword_59"), "").unwrap();
        assert_eq!(nopass.logins.len(), 4);
        // Firefox 114 fixture contains deliberately corrupted entries.
        let corrupted = logins::read(&root.join("test_profile_firefox_nopassword_114"), "").unwrap();
        assert!(corrupted.failed > 0);
        assert!(!corrupted.logins.is_empty());
    }
}
