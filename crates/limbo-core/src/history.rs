//! Browsing history: places + visits, frecency, local search and autofill.

use rusqlite::{Connection, OptionalExtension, params};
use serde::Serialize;
use url::Url;

use crate::error::Result;
use crate::frecency::{self, Transition};
use crate::suggest::{self, LocalMatch};

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HistoryEntry {
    pub visit_id: i64,
    pub place_id: i64,
    pub url: String,
    pub title: String,
    pub visit_us: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TopSite {
    pub url: String,
    pub title: String,
    pub host: String,
}

/// A page and its visits from an importer.
#[derive(Debug, Clone, Default)]
pub struct ImportedPlace {
    pub url: String,
    pub title: Option<String>,
    pub typed_count: i64,
    /// `(visit_us, transition)`
    pub visits: Vec<(i64, Transition)>,
}

/// Only these URLs are recorded in history.
pub fn is_recordable(url: &str) -> bool {
    matches!(Url::parse(url).map(|u| u.scheme().to_string()).as_deref(), Ok("http" | "https" | "file"))
}

fn host_and_prefix(url: &str) -> (String, String) {
    match Url::parse(url) {
        Ok(u) => {
            let host = u.host_str().unwrap_or("").to_ascii_lowercase();
            let host = match u.port() {
                Some(p) => format!("{host}:{p}"),
                None => host,
            };
            (host, format!("{}://", u.scheme()))
        }
        Err(_) => (String::new(), String::new()),
    }
}

/// Inserts the place if missing and returns its id. The title is only set
/// when given (an empty title never overwrites a real one).
pub fn ensure_place(conn: &Connection, url: &str, title: Option<&str>) -> Result<i64> {
    let (host, _) = host_and_prefix(url);
    let title = title.filter(|t| !t.is_empty());
    conn.execute(
        "INSERT INTO places(url, title, host) VALUES (?1, ?2, ?3)
         ON CONFLICT(url) DO UPDATE SET title = COALESCE(excluded.title, places.title)",
        params![url, title, host],
    )?;
    Ok(conn.query_row("SELECT id FROM places WHERE url = ?1", [url], |r| r.get(0))?)
}

/// Records a top-level navigation. Returns the place id (or `None` for URLs
/// that aren't recorded, such as `about:blank`).
pub fn record_visit(
    conn: &mut Connection,
    url: &str,
    title: Option<&str>,
    transition: Transition,
    now_us: i64,
) -> Result<Option<i64>> {
    if !is_recordable(url) {
        return Ok(None);
    }
    let tx = conn.transaction()?;
    let place_id = ensure_place(&tx, url, title)?;
    let inserted = tx.execute(
        "INSERT OR IGNORE INTO visits(place_id, visit_us, transition, source) VALUES (?1, ?2, ?3, 0)",
        params![place_id, now_us, transition as i64],
    )?;
    if inserted > 0 {
        let typed = i64::from(transition == Transition::Typed);
        tx.execute(
            "UPDATE places SET visit_count = visit_count + 1, typed_count = typed_count + ?2,
               last_visit_us = MAX(COALESCE(last_visit_us, 0), ?3)
             WHERE id = ?1",
            params![place_id, typed, now_us],
        )?;
    }
    update_frecency(&tx, place_id, now_us)?;
    let (host, prefix) = host_and_prefix(url);
    update_origin(&tx, &host, &prefix)?;
    tx.commit()?;
    Ok(Some(place_id))
}

/// Updates a page title (DocumentTitleChanged).
pub fn set_title(conn: &Connection, url: &str, title: &str) -> Result<()> {
    if title.is_empty() {
        return Ok(());
    }
    conn.execute("UPDATE places SET title = ?2 WHERE url = ?1 AND COALESCE(title, '') != ?2", params![url, title])?;
    Ok(())
}

/// Recomputes one page's frecency from its most recent visits.
pub fn update_frecency(conn: &Connection, place_id: i64, now_us: i64) -> Result<i64> {
    let mut stmt = conn.prepare_cached(
        "SELECT visit_us, transition FROM visits WHERE place_id = ?1 ORDER BY visit_us DESC LIMIT ?2",
    )?;
    let visits: Vec<(i64, Transition)> = stmt
        .query_map(params![place_id, frecency::SAMPLE_SIZE as i64], |r| {
            Ok((r.get::<_, i64>(0)?, Transition::from_i64(r.get(1)?)))
        })?
        .collect::<Result<_, _>>()?;
    let (visit_count, bookmarked): (i64, bool) = conn.query_row(
        "SELECT visit_count, EXISTS(SELECT 1 FROM bookmarks b WHERE b.url = p.url) FROM places p WHERE id = ?1",
        [place_id],
        |r| Ok((r.get(0)?, r.get(1)?)),
    )?;
    let score = frecency::compute(&visits, visit_count, bookmarked, now_us);
    conn.execute("UPDATE places SET frecency = ?2 WHERE id = ?1", params![place_id, score])?;
    Ok(score)
}

/// An origin's frecency is the sum of its pages' frecency.
pub fn update_origin(conn: &Connection, host: &str, prefix: &str) -> Result<()> {
    if host.is_empty() || prefix == "file://" {
        return Ok(());
    }
    let sum: i64 =
        conn.query_row("SELECT COALESCE(SUM(frecency), 0) FROM places WHERE host = ?1", [host], |r| r.get(0))?;
    if sum <= 0 {
        conn.execute("DELETE FROM origins WHERE host = ?1", [host])?;
    } else {
        conn.execute(
            "INSERT INTO origins(host, prefix, frecency) VALUES (?1, ?2, ?3)
             ON CONFLICT(host) DO UPDATE SET frecency = excluded.frecency,
               prefix = CASE WHEN excluded.prefix = 'https://' THEN 'https://' ELSE origins.prefix END",
            params![host, prefix, sum],
        )?;
    }
    Ok(())
}

/// Recomputes every page's frecency (nightly, on idle). Returns pages updated.
pub fn recompute_all(conn: &mut Connection, now_us: i64) -> Result<usize> {
    let ids: Vec<i64> = {
        let mut stmt = conn.prepare("SELECT id FROM places")?;
        stmt.query_map([], |r| r.get(0))?.collect::<Result<_, _>>()?
    };
    for chunk in ids.chunks(5000) {
        let tx = conn.transaction()?;
        for &id in chunk {
            update_frecency(&tx, id, now_us)?;
        }
        tx.commit()?;
    }
    rebuild_origins(conn)?;
    Ok(ids.len())
}

pub fn rebuild_origins(conn: &mut Connection) -> Result<()> {
    let tx = conn.transaction()?;
    tx.execute("DELETE FROM origins", [])?;
    tx.execute(
        "INSERT INTO origins(host, prefix, frecency)
         SELECT host,
                CASE WHEN SUM(url LIKE 'https://%') > 0 THEN 'https://' ELSE 'http://' END,
                SUM(frecency)
         FROM places
         WHERE host != '' AND (url LIKE 'http://%' OR url LIKE 'https://%')
         GROUP BY host HAVING SUM(frecency) > 0",
        [],
    )?;
    tx.commit()?;
    Ok(())
}

/// Local omnibox matches (history + bookmarks), best first.
pub fn search(conn: &Connection, text: &str, limit: usize) -> Result<Vec<LocalMatch>> {
    let Some(query) = suggest::fts_query(text) else { return Ok(Vec::new()) };
    let mut stmt = conn.prepare_cached(
        "SELECT p.url, COALESCE(p.title, ''), p.frecency,
                EXISTS(SELECT 1 FROM bookmarks b WHERE b.url = p.url)
         FROM places_fts f JOIN places p ON p.id = f.rowid
         WHERE places_fts MATCH ?1 AND p.frecency > 0
         ORDER BY p.frecency DESC
         LIMIT ?2",
    )?;
    let rows = stmt
        .query_map(params![query, limit as i64], |r| {
            Ok(LocalMatch { url: r.get(0)?, title: r.get(1)?, frecency: r.get(2)?, bookmarked: r.get(3)? })
        })?
        .collect::<Result<_, _>>()?;
    Ok(rows)
}

/// Origin candidates (`https://www.google.com`) whose host starts with the
/// typed text, best first. Only host-level input is autofilled.
pub fn autofill_hosts(conn: &Connection, text: &str, limit: usize) -> Result<Vec<String>> {
    let lower = text.trim().to_lowercase();
    let core = suggest::strip_for_autofill(&lower);
    if core.is_empty() || core.contains(['/', ' ', '?', '#']) {
        return Ok(Vec::new());
    }
    let hi = |s: &str| format!("{s}\u{10FFFF}");
    let www = format!("www.{core}");
    let mut stmt = conn.prepare_cached(
        "SELECT prefix || host FROM origins
         WHERE (host >= ?1 AND host < ?2) OR (host >= ?3 AND host < ?4)
         ORDER BY frecency DESC LIMIT ?5",
    )?;
    let rows = stmt
        .query_map(params![core, hi(core), www, hi(&www), limit as i64], |r| r.get(0))?
        .collect::<Result<_, _>>()?;
    Ok(rows)
}

/// History page: most recent visits first, optionally filtered by text.
/// Pass the last `visit_us` as `before_us` to page.
pub fn visits(conn: &Connection, text: Option<&str>, before_us: Option<i64>, limit: usize) -> Result<Vec<HistoryEntry>> {
    let before = before_us.unwrap_or(i64::MAX);
    let map = |r: &rusqlite::Row<'_>| {
        Ok(HistoryEntry {
            visit_id: r.get(0)?,
            place_id: r.get(1)?,
            url: r.get(2)?,
            title: r.get::<_, Option<String>>(3)?.unwrap_or_default(),
            visit_us: r.get(4)?,
        })
    };
    // Downloads (7), embeds (4) and framed links (8) aren't page visits the user made.
    let hidden = "(4, 7, 8)";
    match text.and_then(suggest::fts_query) {
        Some(q) => {
            let sql = format!(
                "SELECT v.id, p.id, p.url, p.title, v.visit_us FROM visits v JOIN places p ON p.id = v.place_id
                 WHERE v.visit_us < ?1 AND v.transition NOT IN {hidden}
                   AND p.id IN (SELECT rowid FROM places_fts WHERE places_fts MATCH ?2)
                 ORDER BY v.visit_us DESC LIMIT ?3"
            );
            let mut stmt = conn.prepare_cached(&sql)?;
            let rows = stmt.query_map(params![before, q, limit as i64], map)?.collect::<Result<_, _>>()?;
            Ok(rows)
        }
        None => {
            let sql = format!(
                "SELECT v.id, p.id, p.url, p.title, v.visit_us FROM visits v JOIN places p ON p.id = v.place_id
                 WHERE v.visit_us < ?1 AND v.transition NOT IN {hidden}
                 ORDER BY v.visit_us DESC LIMIT ?2"
            );
            let mut stmt = conn.prepare_cached(&sql)?;
            let rows = stmt.query_map(params![before, limit as i64], map)?.collect::<Result<_, _>>()?;
            Ok(rows)
        }
    }
}

/// Removes places that have no visits and aren't bookmarked, and refreshes
/// counters/frecency/origins for the given places.
fn refresh_places(conn: &Connection, place_ids: &[i64], now_us: i64) -> Result<()> {
    let mut hosts: Vec<(String, String)> = Vec::new();
    for &id in place_ids {
        let Some(url): Option<String> =
            conn.query_row("SELECT url FROM places WHERE id = ?1", [id], |r| r.get(0)).optional()?
        else {
            continue;
        };
        let hp = host_and_prefix(&url);
        if !hosts.contains(&hp) {
            hosts.push(hp);
        }
        let (count, typed, last): (i64, i64, Option<i64>) = conn.query_row(
            "SELECT COUNT(*), COALESCE(SUM(transition = 2), 0), MAX(visit_us) FROM visits WHERE place_id = ?1",
            [id],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )?;
        let bookmarked: bool =
            conn.query_row("SELECT EXISTS(SELECT 1 FROM bookmarks WHERE url = ?1)", [&url], |r| r.get(0))?;
        if count == 0 && !bookmarked {
            conn.execute("DELETE FROM places WHERE id = ?1", [id])?;
            conn.execute("DELETE FROM page_icons WHERE page_url = ?1", [&url])?;
            continue;
        }
        conn.execute(
            "UPDATE places SET visit_count = ?2, typed_count = ?3, last_visit_us = ?4 WHERE id = ?1",
            params![id, count, typed, last],
        )?;
        update_frecency(conn, id, now_us)?;
    }
    for (host, prefix) in hosts {
        update_origin(conn, &host, &prefix)?;
    }
    Ok(())
}

pub fn delete_visits(conn: &mut Connection, visit_ids: &[i64], now_us: i64) -> Result<()> {
    let tx = conn.transaction()?;
    let mut places = Vec::new();
    for &id in visit_ids {
        if let Some(pid) =
            tx.query_row("SELECT place_id FROM visits WHERE id = ?1", [id], |r| r.get::<_, i64>(0)).optional()?
        {
            tx.execute("DELETE FROM visits WHERE id = ?1", [id])?;
            if !places.contains(&pid) {
                places.push(pid);
            }
        }
    }
    refresh_places(&tx, &places, now_us)?;
    tx.commit()?;
    Ok(())
}

/// Shift+Delete on a suggestion: forget a page entirely.
pub fn delete_url(conn: &mut Connection, url: &str, now_us: i64) -> Result<()> {
    let tx = conn.transaction()?;
    if let Some(pid) = tx.query_row("SELECT id FROM places WHERE url = ?1", [url], |r| r.get::<_, i64>(0)).optional()? {
        tx.execute("DELETE FROM visits WHERE place_id = ?1", [pid])?;
        refresh_places(&tx, &[pid], now_us)?;
    }
    tx.commit()?;
    Ok(())
}

/// "Clear browsing data" for a time range (`to_us` exclusive).
pub fn clear_range(conn: &mut Connection, from_us: i64, to_us: i64, now_us: i64) -> Result<usize> {
    let tx = conn.transaction()?;
    let places: Vec<i64> = {
        let mut stmt =
            tx.prepare("SELECT DISTINCT place_id FROM visits WHERE visit_us >= ?1 AND visit_us < ?2")?;
        stmt.query_map(params![from_us, to_us], |r| r.get(0))?.collect::<Result<_, _>>()?
    };
    let removed = tx.execute("DELETE FROM visits WHERE visit_us >= ?1 AND visit_us < ?2", params![from_us, to_us])?;
    refresh_places(&tx, &places, now_us)?;
    tx.commit()?;
    Ok(removed)
}

/// New-tab tiles: the most frecent sites, one per host, excluding search results.
pub fn top_sites(conn: &Connection, limit: usize) -> Result<Vec<TopSite>> {
    let mut stmt = conn.prepare_cached(
        "SELECT prefix, host FROM origins ORDER BY frecency DESC LIMIT ?1",
    )?;
    let rows: Vec<(String, String)> =
        stmt.query_map([(limit * 3) as i64], |r| Ok((r.get(0)?, r.get(1)?)))?.collect::<Result<_, _>>()?;
    let mut out: Vec<TopSite> = Vec::new();
    for (prefix, host) in rows {
        if out.len() >= limit {
            break;
        }
        let bare = host.strip_prefix("www.").unwrap_or(&host).to_string();
        if out.iter().any(|s| s.host == bare) {
            continue;
        }
        let title: Option<String> = conn
            .query_row(
                "SELECT title FROM places WHERE host = ?1 AND title IS NOT NULL AND title != ''
                 ORDER BY (url = ?2) DESC, frecency DESC LIMIT 1",
                params![host, format!("{prefix}{host}/")],
                |r| r.get(0),
            )
            .optional()?;
        out.push(TopSite { url: format!("{prefix}{host}/"), title: title.unwrap_or_else(|| bare.clone()), host: bare });
    }
    Ok(out)
}

/// Bulk import (Firefox). Idempotent: visits are unique per (page, time).
/// Returns `(places, visits)` inserted.
pub fn import_places(conn: &mut Connection, places: &[ImportedPlace], source: i64, now_us: i64) -> Result<(usize, usize)> {
    let mut new_places = 0usize;
    let mut new_visits = 0usize;
    let mut touched: Vec<i64> = Vec::with_capacity(places.len());
    for chunk in places.chunks(5000) {
        let tx = conn.transaction()?;
        {
            let mut exists = tx.prepare_cached("SELECT id FROM places WHERE url = ?1")?;
            let mut insert_visit = tx.prepare_cached(
                "INSERT OR IGNORE INTO visits(place_id, visit_us, transition, source) VALUES (?1, ?2, ?3, ?4)",
            )?;
            for p in chunk {
                if !is_recordable(&p.url) {
                    continue;
                }
                let existed: Option<i64> = exists.query_row([&p.url], |r| r.get(0)).optional()?;
                let id = match existed {
                    Some(id) => {
                        if let Some(t) = p.title.as_deref().filter(|t| !t.is_empty()) {
                            tx.execute("UPDATE places SET title = ?2 WHERE id = ?1 AND title IS NULL", params![id, t])?;
                        }
                        id
                    }
                    None => {
                        new_places += 1;
                        ensure_place(&tx, &p.url, p.title.as_deref())?
                    }
                };
                for &(t, tr) in &p.visits {
                    new_visits += insert_visit.execute(params![id, t, tr as i64, source])?;
                }
                touched.push(id);
            }
        }
        tx.commit()?;
    }
    for chunk in touched.chunks(5000) {
        let tx = conn.transaction()?;
        for &id in chunk {
            let (count, typed, last): (i64, i64, Option<i64>) = tx.query_row(
                "SELECT COUNT(*), COALESCE(SUM(transition = 2), 0), MAX(visit_us) FROM visits WHERE place_id = ?1",
                [id],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )?;
            tx.execute(
                "UPDATE places SET visit_count = ?2, typed_count = MAX(typed_count, ?3), last_visit_us = ?4 WHERE id = ?1",
                params![id, count, typed, last],
            )?;
            update_frecency(&tx, id, now_us)?;
        }
        tx.commit()?;
    }
    rebuild_origins(conn)?;
    Ok((new_places, new_visits))
}

pub fn place_count(conn: &Connection) -> Result<i64> {
    Ok(conn.query_row("SELECT COUNT(*) FROM places", [], |r| r.get(0))?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::Db;
    use crate::frecency::US_PER_DAY;

    const NOW: i64 = 1_800_000_000_000_000;

    fn db() -> Db {
        Db::open_in_memory().unwrap()
    }

    #[test]
    fn records_visits_and_frecency() {
        let mut db = db();
        let c = db.conn_mut();
        let id = record_visit(c, "https://www.rust-lang.org/", Some("Rust"), Transition::Typed, NOW).unwrap().unwrap();
        record_visit(c, "https://www.rust-lang.org/", None, Transition::Link, NOW + 1).unwrap();
        assert!(record_visit(c, "about:blank", None, Transition::Link, NOW).unwrap().is_none());
        let (count, typed, title, fr): (i64, i64, String, i64) = c
            .query_row("SELECT visit_count, typed_count, title, frecency FROM places WHERE id = ?1", [id], |r| {
                Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?))
            })
            .unwrap();
        assert_eq!((count, typed, title.as_str()), (2, 1, "Rust"));
        assert_eq!(fr, 300);
        let origin: (String, i64) =
            c.query_row("SELECT prefix || host, frecency FROM origins", [], |r| Ok((r.get(0)?, r.get(1)?))).unwrap();
        assert_eq!(origin, ("https://www.rust-lang.org".into(), 300));
    }

    #[test]
    fn search_and_autofill() {
        let mut db = db();
        let c = db.conn_mut();
        record_visit(c, "https://www.rust-lang.org/", Some("Rust Programming Language"), Transition::Typed, NOW).unwrap();
        record_visit(c, "https://doc.rust-lang.org/book/", Some("The Rust Book"), Transition::Link, NOW).unwrap();
        record_visit(c, "https://github.com/rust-lang/rust", Some("rust-lang/rust"), Transition::Link, NOW - 100 * US_PER_DAY).unwrap();
        record_visit(c, "https://www.google.com/", Some("Google"), Transition::Link, NOW).unwrap();

        let m = search(c, "rust", 10).unwrap();
        assert_eq!(m.len(), 3);
        assert_eq!(m[0].url, "https://www.rust-lang.org/", "typed visit ranks first");
        assert_eq!(search(c, "rust book", 10).unwrap().len(), 1);
        assert_eq!(search(c, "\"", 10).unwrap().len(), 0, "quotes are inert");

        assert_eq!(autofill_hosts(c, "goo", 5).unwrap(), vec!["https://www.google.com"]);
        assert_eq!(autofill_hosts(c, "www.goo", 5).unwrap(), vec!["https://www.google.com"]);
        assert_eq!(autofill_hosts(c, "https://git", 5).unwrap(), vec!["https://github.com"]);
        assert!(autofill_hosts(c, "github.com/rust", 5).unwrap().is_empty());
        assert!(autofill_hosts(c, "zzz", 5).unwrap().is_empty());
    }

    #[test]
    fn delete_and_clear() {
        let mut db = db();
        let c = db.conn_mut();
        record_visit(c, "https://a.com/", Some("A"), Transition::Link, NOW - 10).unwrap();
        record_visit(c, "https://a.com/", Some("A"), Transition::Link, NOW).unwrap();
        record_visit(c, "https://b.com/", Some("B"), Transition::Link, NOW - 5).unwrap();
        let v = visits(c, None, None, 10).unwrap();
        assert_eq!(v.iter().map(|e| e.url.as_str()).collect::<Vec<_>>(), vec!["https://a.com/", "https://b.com/", "https://a.com/"]);
        let page2 = visits(c, None, Some(v[1].visit_us), 10).unwrap();
        assert_eq!(page2.len(), 1);
        assert_eq!(visits(c, Some("b.com"), None, 10).unwrap().len(), 1);

        delete_visits(c, &[v[0].visit_id], NOW).unwrap();
        let count: i64 = c.query_row("SELECT visit_count FROM places WHERE url='https://a.com/'", [], |r| r.get(0)).unwrap();
        assert_eq!(count, 1);

        delete_url(c, "https://b.com/", NOW).unwrap();
        assert_eq!(place_count(c).unwrap(), 1);
        assert!(autofill_hosts(c, "b.", 5).unwrap().is_empty(), "origin removed with its last page");

        clear_range(c, NOW - 100, NOW + 100, NOW).unwrap();
        assert_eq!(place_count(c).unwrap(), 0);
    }

    #[test]
    fn top_sites_one_per_host() {
        let mut db = db();
        let c = db.conn_mut();
        for i in 0..5 {
            record_visit(c, &format!("https://www.example.com/p{i}"), Some("Example page"), Transition::Link, NOW - i).unwrap();
        }
        record_visit(c, "https://news.ycombinator.com/", Some("Hacker News"), Transition::Typed, NOW).unwrap();
        let sites = top_sites(c, 8).unwrap();
        assert_eq!(sites.len(), 2);
        assert_eq!(sites[0].host, "example.com");
        assert_eq!(sites[0].url, "https://www.example.com/");
        assert_eq!(sites[1].title, "Hacker News");
    }

    #[test]
    fn import_is_idempotent() {
        let mut db = db();
        let c = db.conn_mut();
        let places = vec![
            ImportedPlace {
                url: "https://mozilla.org/".into(),
                title: Some("Mozilla".into()),
                typed_count: 1,
                visits: vec![(NOW - 1000, Transition::Typed), (NOW - 500, Transition::Link)],
            },
            ImportedPlace { url: "place:sort=8".into(), title: None, typed_count: 0, visits: vec![(NOW, Transition::Link)] },
        ];
        assert_eq!(import_places(c, &places, 1, NOW).unwrap(), (1, 2));
        assert_eq!(import_places(c, &places, 1, NOW).unwrap(), (0, 0));
        let (count, fr): (i64, i64) =
            c.query_row("SELECT visit_count, frecency FROM places", [], |r| Ok((r.get(0)?, r.get(1)?))).unwrap();
        assert_eq!(count, 2);
        assert!(fr > 0);
        assert_eq!(autofill_hosts(c, "moz", 3).unwrap(), vec!["https://mozilla.org"]);
    }

    /// Phase 4 gate: local suggestions in <= 30 ms with 100k history rows.
    /// `cargo test -p limbo-core --release -- --ignored suggestions_are_fast`
    #[test]
    #[ignore]
    fn suggestions_are_fast_with_100k_rows() {
        let mut db = db();
        let words = ["rust", "google", "mail", "news", "docs", "video", "shop", "maps", "wiki", "code"];
        let places: Vec<ImportedPlace> = (0..100_000)
            .map(|i| ImportedPlace {
                url: format!("https://{}{}.example{}.com/page/{i}", words[i % 10], i % 97, i % 13),
                title: Some(format!("{} page {} {}", words[(i / 10) % 10], i, words[(i / 100) % 10])),
                typed_count: 0,
                visits: vec![(NOW - (i as i64 % 400) * US_PER_DAY / 4, Transition::Link)],
            })
            .collect();
        import_places(db.conn_mut(), &places, 1, NOW).unwrap();
        let c = db.conn();
        for q in ["r", "ru", "rust", "google page", "news 12", "example3.com", "zzz"] {
            let start = std::time::Instant::now();
            let _ = search(c, q, 8).unwrap();
            let _ = autofill_hosts(c, q, 3).unwrap();
            let ms = start.elapsed().as_secs_f64() * 1000.0;
            println!("{q:>14}: {ms:.2} ms");
            assert!(ms <= 30.0, "{q}: {ms} ms");
        }
    }
}
