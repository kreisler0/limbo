//! Builds a synthetic Firefox profile with Firefox's own table layouts and
//! known values, so every importer can be tested end to end.
//!
//! Real Firefox-generated profiles are exercised by `real_firefox_profiles`
//! (see tools/fetch-firefox-testdata.sh).

use std::path::Path;

use rusqlite::{Connection, params};

use super::key4::testutil;

pub const HISTORY_PAGES: usize = 4;
pub const HISTORY_VISITS: usize = 8;
pub const BOOKMARKS: usize = 6;
pub const LOGINS: usize = 3;

const DAY_US: i64 = 86_400_000_000;

pub fn build(profile: &Path, primary_password: &str) {
    let now = crate::db::now_us();
    places(profile, now);
    favicons(profile);
    logins(profile, primary_password);
    cookies(profile, now / 1_000_000);
    formhistory(profile);
    session(profile);
    std::fs::write(
        profile.join("extensions.json"),
        r#"{"addons":[
          {"id":"uBlock0@raymondhill.net","type":"extension","location":"app-profile","active":true,"defaultLocale":{"name":"uBlock Origin"}},
          {"id":"addon@darkreader.org","type":"extension","location":"app-profile","active":true,"defaultLocale":{"name":"Dark Reader"}},
          {"id":"pictureinpicture@mozilla.org","type":"extension","location":"app-system-defaults","active":true}
        ]}"#,
    )
    .unwrap();
}

fn places(profile: &Path, now: i64) {
    let c = Connection::open(profile.join("places.sqlite")).unwrap();
    c.execute_batch(
        "PRAGMA journal_mode=WAL;
         CREATE TABLE moz_places (id INTEGER PRIMARY KEY, url LONGVARCHAR, title LONGVARCHAR, rev_host LONGVARCHAR,
           visit_count INTEGER DEFAULT 0, hidden INTEGER DEFAULT 0 NOT NULL, typed INTEGER DEFAULT 0 NOT NULL,
           frecency INTEGER DEFAULT -1 NOT NULL, last_visit_date INTEGER, guid TEXT, foreign_count INTEGER DEFAULT 0 NOT NULL,
           url_hash INTEGER DEFAULT 0 NOT NULL, description TEXT, preview_image_url TEXT, origin_id INTEGER);
         CREATE TABLE moz_historyvisits (id INTEGER PRIMARY KEY, from_visit INTEGER, place_id INTEGER, visit_date INTEGER,
           visit_type INTEGER, session INTEGER, source INTEGER DEFAULT 0 NOT NULL, triggeringPlaceId INTEGER);
         CREATE TABLE moz_bookmarks (id INTEGER PRIMARY KEY, type INTEGER, fk INTEGER DEFAULT NULL, parent INTEGER,
           position INTEGER, title LONGVARCHAR, keyword_id INTEGER, folder_type TEXT, dateAdded INTEGER,
           lastModified INTEGER, guid TEXT, syncStatus INTEGER NOT NULL DEFAULT 0, syncChangeCounter INTEGER NOT NULL DEFAULT 1);",
    )
    .unwrap();
    let pages: &[(i64, &str, Option<&str>, i64)] = &[
        (1, "https://www.rust-lang.org/", Some("Rust"), 1),
        (2, "https://doc.rust-lang.org/book/", Some("The Book"), 0),
        (3, "https://developer.mozilla.org/", Some("MDN"), 0),
        (4, "place:sort=8&maxResults=10", None, 0),
        (5, "about:config", None, 0),
        (6, "https://github.com/login", Some("GitHub"), 0),
        (7, "https://news.ycombinator.com/", Some("Hacker News"), 0),
    ];
    for &(id, url, title, typed) in pages {
        c.execute(
            "INSERT INTO moz_places(id, url, title, typed) VALUES (?1, ?2, ?3, ?4)",
            params![id, url, title, typed],
        )
        .unwrap();
    }
    let visits: &[(i64, i64, i64)] = &[
        (1, now - DAY_US, 2),
        (1, now - 2 * DAY_US, 1),
        (1, now - 40 * DAY_US, 1),
        (2, now - DAY_US, 1),
        (2, now - 3 * DAY_US, 1),
        (6, now - 5 * DAY_US, 1),
        (7, now - 1000, 2),
        (7, now - 2000, 1),
        (4, now, 1),
        (5, now, 2),
    ];
    for &(pid, t, ty) in visits {
        c.execute(
            "INSERT INTO moz_historyvisits(place_id, visit_date, visit_type) VALUES (?1, ?2, ?3)",
            params![pid, t, ty],
        )
        .unwrap();
    }
    // id, type, fk, parent, position, title, guid
    type Row<'a> = (i64, i64, Option<i64>, i64, i64, &'a str, &'a str);
    let bookmarks: &[Row] = &[
        (1, 2, None, 0, 0, "", "root________"),
        (2, 2, None, 1, 0, "menu", "menu________"),
        (3, 2, None, 1, 1, "toolbar", "toolbar_____"),
        (4, 2, None, 1, 2, "tags", "tags________"),
        (5, 2, None, 1, 3, "unfiled", "unfiled_____"),
        (6, 2, None, 1, 4, "mobile", "mobile______"),
        (7, 2, None, 3, 0, "Dev", "devfolder001"),
        (8, 1, Some(1), 7, 0, "Rust", "rustbookmark"),
        (9, 1, Some(3), 3, 1, "MDN", "mdnbookmark1"),
        (10, 3, None, 3, 2, "", "separator001"),
        (11, 1, Some(4), 3, 3, "Most Visited", "smartquery01"),
        (12, 2, None, 4, 0, "rust", "tagfolder001"),
        (13, 1, Some(7), 2, 0, "HN", "hnbookmark01"),
        (14, 1, Some(6), 5, 0, "GitHub", "ghbookmark01"),
    ];
    for &(id, ty, fk, parent, pos, title, guid) in bookmarks {
        c.execute(
            "INSERT INTO moz_bookmarks(id, type, fk, parent, position, title, dateAdded, lastModified, guid)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, 1600000000000000, 1600000000000000, ?7)",
            params![id, ty, fk, parent, pos, title, guid],
        )
        .unwrap();
    }
    // Leave the connection open until here so some rows live in the WAL.
    drop(c);
}

fn favicons(profile: &Path) {
    let c = Connection::open(profile.join("favicons.sqlite")).unwrap();
    c.execute_batch(
        "CREATE TABLE moz_icons (id INTEGER PRIMARY KEY, icon_url TEXT NOT NULL, fixed_icon_url_hash INTEGER NOT NULL,
           width INTEGER NOT NULL DEFAULT 0, root INTEGER NOT NULL DEFAULT 0, color INTEGER, expire_ms INTEGER NOT NULL DEFAULT 0, data BLOB);
         CREATE TABLE moz_pages_w_icons (id INTEGER PRIMARY KEY, page_url TEXT NOT NULL, page_url_hash INTEGER NOT NULL);
         CREATE TABLE moz_icons_to_pages (page_id INTEGER NOT NULL, icon_id INTEGER NOT NULL, expire_ms INTEGER NOT NULL DEFAULT 0,
           PRIMARY KEY (page_id, icon_id));",
    )
    .unwrap();
    let png = |w: u8| [b"\x89PNG\r\n\x1a\n".to_vec(), vec![w]].concat();
    let icons: &[(i64, &str, i64, Vec<u8>)] = &[
        (1, "https://www.rust-lang.org/favicon-16.png", 16, png(16)),
        (2, "https://www.rust-lang.org/favicon-32.png", 32, png(32)),
        (3, "https://www.rust-lang.org/favicon-128.png", 128, png(128)),
        (4, "https://developer.mozilla.org/favicon.svg", 65535, b"<svg xmlns='http://www.w3.org/2000/svg'/>".to_vec()),
        (5, "https://unknown.example/favicon.ico", 16, vec![0, 0, 1, 0]),
    ];
    for (id, url, w, data) in icons {
        c.execute(
            "INSERT INTO moz_icons(id, icon_url, fixed_icon_url_hash, width, data) VALUES (?1, ?2, 0, ?3, ?4)",
            params![id, url, w, data],
        )
        .unwrap();
    }
    let pages: &[(i64, &str, &[i64])] = &[
        (1, "https://www.rust-lang.org/", &[1, 2, 3]),
        (2, "https://developer.mozilla.org/", &[4]),
        (3, "https://unknown.example/", &[5]),
    ];
    for (id, url, icon_ids) in pages {
        c.execute("INSERT INTO moz_pages_w_icons(id, page_url, page_url_hash) VALUES (?1, ?2, 0)", params![id, url])
            .unwrap();
        for icon in *icon_ids {
            c.execute("INSERT INTO moz_icons_to_pages(page_id, icon_id) VALUES (?1, ?2)", params![id, icon]).unwrap();
        }
    }
}

fn logins(profile: &Path, primary_password: &str) {
    let key = [0x5Cu8; 32];
    testutil::make_key4(&profile.join("key4.db"), primary_password, &key);
    let aes = |s: &str| testutil::login_value_aes(&key, &[3u8; 16], s);
    let tdes = |s: &str| testutil::login_value_3des(&key[..24], &[4u8; 8], s);
    let entry = |host: &str, u: String, p: String, guid: &str| {
        serde_json::json!({
            "id": 1, "hostname": host, "httpRealm": null, "formSubmitURL": host,
            "usernameField": "login", "passwordField": "password",
            "encryptedUsername": u, "encryptedPassword": p, "guid": guid, "encType": 1,
            "timeCreated": 1_600_000_000_000i64, "timeLastUsed": 1_600_000_000_000i64,
            "timePasswordChanged": 1_600_000_000_000i64, "timesUsed": 3
        })
    };
    let json = serde_json::json!({
        "nextId": 6,
        "logins": [
            entry("https://github.com", aes("octocat"), aes("hunter2"), "{g1}"),
            entry("https://accounts.google.com", aes("me@gmail.com"), aes("p@ss w0rd ✓"), "{g2}"),
            entry("http://intranet.local", tdes("admin"), tdes("admin"), "{g3}"),
            entry("chrome://FirefoxAccounts", aes("sync"), aes("secret"), "{g4}"),
            entry("https://broken.example", "MDoEEPgAAAAAAAAAAAAAAAAAAAE=".into(), aes("x"), "{g5}"),
        ],
        "version": 3
    });
    std::fs::write(profile.join("logins.json"), serde_json::to_string_pretty(&json).unwrap()).unwrap();
}

fn cookies(profile: &Path, now_s: i64) {
    let c = Connection::open(profile.join("cookies.sqlite")).unwrap();
    c.execute_batch(
        "CREATE TABLE moz_cookies (id INTEGER PRIMARY KEY, originAttributes TEXT NOT NULL DEFAULT '', name TEXT, value TEXT,
           host TEXT, path TEXT, expiry INTEGER, lastAccessed INTEGER, creationTime INTEGER, isSecure INTEGER,
           isHttpOnly INTEGER, inBrowserElement INTEGER DEFAULT 0, sameSite INTEGER DEFAULT 0, rawSameSite INTEGER DEFAULT 0,
           schemeMap INTEGER DEFAULT 0);",
    )
    .unwrap();
    let rows: &[(&str, &str, &str, i64, i64)] = &[
        ("", "SID", ".google.com", now_s + 86_400, 1),
        ("", "session", "github.com", (now_s + 86_400) * 1000, 2), // milliseconds
        ("", "old", ".example.com", now_s - 10, 0),
        ("^userContextId=1", "work", ".example.com", now_s + 86_400, 0),
    ];
    for (oa, name, host, expiry, same_site) in rows {
        c.execute(
            "INSERT INTO moz_cookies(originAttributes, name, value, host, path, expiry, isSecure, isHttpOnly, sameSite)
             VALUES (?1, ?2, 'v', ?3, '/', ?4, 1, 1, ?5)",
            params![oa, name, host, expiry, same_site],
        )
        .unwrap();
    }
}

fn formhistory(profile: &Path) {
    let c = Connection::open(profile.join("formhistory.sqlite")).unwrap();
    c.execute_batch(
        "CREATE TABLE moz_formhistory (id INTEGER PRIMARY KEY, fieldname TEXT NOT NULL, value TEXT NOT NULL,
           timesUsed INTEGER, firstUsed INTEGER, lastUsed INTEGER, guid TEXT);
         INSERT INTO moz_formhistory(fieldname, value, timesUsed, firstUsed, lastUsed) VALUES
           ('email', 'me@example.com', 4, 1, 2), ('q', 'rust', 1, 1, 1), ('q', '', 1, 1, 1);",
    )
    .unwrap();
}

fn session(profile: &Path) {
    let json = r#"{"selectedWindow":1,"windows":[{"selected":1,"tabs":[
      {"entries":[{"url":"https://www.rust-lang.org/","title":"Rust"}],"index":1,"pinned":true},
      {"entries":[{"url":"https://github.com/","title":"GitHub"}],"index":1},
      {"entries":[{"url":"about:preferences"}],"index":1},
      {"entries":[{"url":"https://news.ycombinator.com/","title":"HN"}],"index":1}
    ]}]}"#;
    std::fs::create_dir_all(profile.join("sessionstore-backups")).unwrap();
    std::fs::write(
        profile.join("sessionstore-backups").join("recovery.jsonlz4"),
        super::session::encode_mozlz4(json.as_bytes()),
    )
    .unwrap();
}
