//! Bookmarks: a tree under Firefox-compatible roots, Netscape HTML import/export.

use rusqlite::{Connection, OptionalExtension, params};
use serde::{Deserialize, Serialize};

use crate::db::roots;
use crate::error::{Error, Result};
use crate::history;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum BookmarkKind {
    Url = 0,
    Folder = 1,
    Separator = 2,
}

impl BookmarkKind {
    fn from_i64(v: i64) -> BookmarkKind {
        match v {
            1 => BookmarkKind::Folder,
            2 => BookmarkKind::Separator,
            _ => BookmarkKind::Url,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Bookmark {
    pub id: i64,
    pub parent_id: Option<i64>,
    pub kind: BookmarkKind,
    pub title: String,
    pub url: Option<String>,
    pub position: i64,
    pub added_us: i64,
    pub modified_us: i64,
    pub guid: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub children: Vec<Bookmark>,
}

/// One item from an importer; parents must come before their children.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImportedBookmark {
    pub guid: String,
    pub parent_guid: String,
    pub kind: BookmarkKind,
    pub title: String,
    pub url: Option<String>,
    pub added_us: i64,
    pub modified_us: i64,
}

const ROOT_TITLES: &[(&str, &str)] = &[
    (roots::MENU, "Bookmarks menu"),
    (roots::TOOLBAR, "Bookmarks bar"),
    (roots::OTHER, "Other bookmarks"),
    (roots::MOBILE, "Mobile bookmarks"),
];

pub(crate) fn create_roots(conn: &Connection) -> Result<()> {
    let now = crate::db::now_us();
    conn.execute(
        "INSERT OR IGNORE INTO bookmarks(id, parent_id, kind, title, position, added_us, modified_us, guid)
         VALUES (1, NULL, 1, '', 0, ?1, ?1, ?2)",
        params![now, roots::ROOT],
    )?;
    for (i, (guid, title)) in ROOT_TITLES.iter().enumerate() {
        conn.execute(
            "INSERT OR IGNORE INTO bookmarks(parent_id, kind, title, position, added_us, modified_us, guid)
             VALUES (1, 1, ?1, ?2, ?3, ?3, ?4)",
            params![title, i as i64, now, guid],
        )?;
    }
    Ok(())
}

/// A random 12-character GUID (the length Firefox uses).
pub fn new_guid() -> String {
    use base64::Engine;
    let bytes = uuid::Uuid::new_v4().into_bytes();
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(&bytes[..9])
}

fn is_root_guid(guid: &str) -> bool {
    guid == roots::ROOT || ROOT_TITLES.iter().any(|(g, _)| *g == guid)
}

pub fn id_for_guid(conn: &Connection, guid: &str) -> Result<Option<i64>> {
    Ok(conn.query_row("SELECT id FROM bookmarks WHERE guid = ?1", [guid], |r| r.get(0)).optional()?)
}

pub fn root_id(conn: &Connection, guid: &str) -> Result<i64> {
    id_for_guid(conn, guid)?.ok_or_else(|| Error::NotFound(format!("bookmark root {guid}")))
}

fn row_to_bookmark(r: &rusqlite::Row<'_>) -> rusqlite::Result<Bookmark> {
    Ok(Bookmark {
        id: r.get(0)?,
        parent_id: r.get(1)?,
        kind: BookmarkKind::from_i64(r.get(2)?),
        title: r.get(3)?,
        url: r.get(4)?,
        position: r.get(5)?,
        added_us: r.get(6)?,
        modified_us: r.get(7)?,
        guid: r.get(8)?,
        children: Vec::new(),
    })
}

const COLUMNS: &str = "id, parent_id, kind, title, url, position, added_us, modified_us, guid";

pub fn get(conn: &Connection, id: i64) -> Result<Option<Bookmark>> {
    Ok(conn
        .query_row(&format!("SELECT {COLUMNS} FROM bookmarks WHERE id = ?1"), [id], row_to_bookmark)
        .optional()?)
}

pub fn children(conn: &Connection, parent_id: i64) -> Result<Vec<Bookmark>> {
    let mut stmt =
        conn.prepare_cached(&format!("SELECT {COLUMNS} FROM bookmarks WHERE parent_id = ?1 ORDER BY position"))?;
    let rows = stmt.query_map([parent_id], row_to_bookmark)?.collect::<Result<_, _>>()?;
    Ok(rows)
}

/// The subtree rooted at `id`.
pub fn tree(conn: &Connection, id: i64) -> Result<Bookmark> {
    let mut node = get(conn, id)?.ok_or_else(|| Error::NotFound(format!("bookmark {id}")))?;
    fill_children(conn, &mut node)?;
    Ok(node)
}

fn fill_children(conn: &Connection, node: &mut Bookmark) -> Result<()> {
    if node.kind == BookmarkKind::Folder {
        node.children = children(conn, node.id)?;
        for c in node.children.iter_mut() {
            fill_children(conn, c)?;
        }
    }
    Ok(())
}

/// Bookmarks for a URL (the omnibox star).
pub fn find_by_url(conn: &Connection, url: &str) -> Result<Vec<Bookmark>> {
    let mut stmt = conn.prepare_cached(&format!("SELECT {COLUMNS} FROM bookmarks WHERE url = ?1 ORDER BY id"))?;
    let rows = stmt.query_map([url], row_to_bookmark)?.collect::<Result<_, _>>()?;
    Ok(rows)
}

fn child_count(conn: &Connection, parent_id: i64) -> Result<i64> {
    Ok(conn.query_row("SELECT COUNT(*) FROM bookmarks WHERE parent_id = ?1", [parent_id], |r| r.get(0))?)
}

fn refresh_place(conn: &Connection, url: &str, title: Option<&str>) -> Result<()> {
    if history::is_recordable(url) {
        let id = history::ensure_place(conn, url, title)?;
        history::update_frecency(conn, id, crate::db::now_us())?;
    }
    Ok(())
}

/// Inserts an item under `parent_id` at `index` (append when `None` or past the end).
pub fn insert(
    conn: &Connection,
    parent_id: i64,
    kind: BookmarkKind,
    title: &str,
    url: Option<&str>,
    index: Option<i64>,
) -> Result<i64> {
    let now = crate::db::now_us();
    insert_full(conn, parent_id, kind, title, url, index, &new_guid(), now, now)
}

#[allow(clippy::too_many_arguments)]
fn insert_full(
    conn: &Connection,
    parent_id: i64,
    kind: BookmarkKind,
    title: &str,
    url: Option<&str>,
    index: Option<i64>,
    guid: &str,
    added_us: i64,
    modified_us: i64,
) -> Result<i64> {
    let parent = get(conn, parent_id)?.ok_or_else(|| Error::NotFound(format!("folder {parent_id}")))?;
    if parent.kind != BookmarkKind::Folder {
        return Err(Error::Format("bookmarks can only be added to folders".into()));
    }
    if kind == BookmarkKind::Url && url.is_none() {
        return Err(Error::Format("a bookmark needs a URL".into()));
    }
    let count = child_count(conn, parent_id)?;
    let pos = index.filter(|&i| i >= 0 && i < count).unwrap_or(count);
    conn.execute(
        "UPDATE bookmarks SET position = position + 1 WHERE parent_id = ?1 AND position >= ?2",
        params![parent_id, pos],
    )?;
    let url = if kind == BookmarkKind::Url { url } else { None };
    conn.execute(
        "INSERT INTO bookmarks(parent_id, kind, title, url, position, added_us, modified_us, guid)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
        params![parent_id, kind as i64, title, url, pos, added_us, modified_us, guid],
    )?;
    let id = conn.last_insert_rowid();
    if let Some(u) = url {
        refresh_place(conn, u, Some(title))?;
    }
    Ok(id)
}

/// Renames and/or re-targets an item.
pub fn update(conn: &Connection, id: i64, title: Option<&str>, url: Option<&str>) -> Result<()> {
    let item = get(conn, id)?.ok_or_else(|| Error::NotFound(format!("bookmark {id}")))?;
    if is_root_guid(&item.guid) {
        return Err(Error::Unsupported("roots can't be edited".into()));
    }
    let now = crate::db::now_us();
    if let Some(t) = title {
        conn.execute("UPDATE bookmarks SET title = ?2, modified_us = ?3 WHERE id = ?1", params![id, t, now])?;
    }
    if let (Some(u), BookmarkKind::Url) = (url, item.kind) {
        conn.execute("UPDATE bookmarks SET url = ?2, modified_us = ?3 WHERE id = ?1", params![id, u, now])?;
        if let Some(old) = &item.url {
            refresh_place(conn, old, None)?;
        }
        refresh_place(conn, u, title)?;
    }
    Ok(())
}

fn is_descendant(conn: &Connection, candidate: i64, ancestor: i64) -> Result<bool> {
    let mut cur = Some(candidate);
    while let Some(id) = cur {
        if id == ancestor {
            return Ok(true);
        }
        cur = conn.query_row("SELECT parent_id FROM bookmarks WHERE id = ?1", [id], |r| r.get(0)).optional()?.flatten();
    }
    Ok(false)
}

/// Moves an item to `new_parent` at `index` (drag and drop).
pub fn move_to(conn: &mut Connection, id: i64, new_parent: i64, index: Option<i64>) -> Result<()> {
    let tx = conn.transaction()?;
    let item = get(&tx, id)?.ok_or_else(|| Error::NotFound(format!("bookmark {id}")))?;
    if is_root_guid(&item.guid) {
        return Err(Error::Unsupported("roots can't be moved".into()));
    }
    if is_descendant(&tx, new_parent, id)? {
        return Err(Error::Unsupported("a folder can't move into itself".into()));
    }
    let old_parent = item.parent_id.unwrap_or(1);
    // Close the gap, then open a slot.
    tx.execute(
        "UPDATE bookmarks SET position = position - 1 WHERE parent_id = ?1 AND position > ?2",
        params![old_parent, item.position],
    )?;
    tx.execute("UPDATE bookmarks SET parent_id = NULL, position = -1 WHERE id = ?1", [id])?;
    let count = child_count(&tx, new_parent)?;
    let pos = index.filter(|&i| i >= 0 && i < count).unwrap_or(count);
    tx.execute(
        "UPDATE bookmarks SET position = position + 1 WHERE parent_id = ?1 AND position >= ?2",
        params![new_parent, pos],
    )?;
    tx.execute(
        "UPDATE bookmarks SET parent_id = ?2, position = ?3, modified_us = ?4 WHERE id = ?1",
        params![id, new_parent, pos, crate::db::now_us()],
    )?;
    tx.commit()?;
    Ok(())
}

/// Removes an item (and its subtree). Returns the removed subtree for undo.
pub fn remove(conn: &mut Connection, id: i64) -> Result<Bookmark> {
    let tx = conn.transaction()?;
    let subtree = tree(&tx, id)?;
    if is_root_guid(&subtree.guid) {
        return Err(Error::Unsupported("roots can't be removed".into()));
    }
    tx.execute("DELETE FROM bookmarks WHERE id = ?1", [id])?;
    if let Some(parent) = subtree.parent_id {
        tx.execute(
            "UPDATE bookmarks SET position = position - 1 WHERE parent_id = ?1 AND position > ?2",
            params![parent, subtree.position],
        )?;
    }
    let mut urls = Vec::new();
    collect_urls(&subtree, &mut urls);
    for u in urls {
        refresh_place(&tx, &u, None)?;
    }
    tx.commit()?;
    Ok(subtree)
}

fn collect_urls(node: &Bookmark, out: &mut Vec<String>) {
    if let Some(u) = &node.url {
        out.push(u.clone());
    }
    for c in &node.children {
        collect_urls(c, out);
    }
}

/// Undo for [`remove`]: puts the subtree back where it was, with the same GUIDs.
pub fn restore(conn: &mut Connection, subtree: &Bookmark) -> Result<i64> {
    let tx = conn.transaction()?;
    let parent = subtree.parent_id.ok_or_else(|| Error::Unsupported("can't restore a root".into()))?;
    let id = restore_node(&tx, subtree, parent, Some(subtree.position))?;
    tx.commit()?;
    Ok(id)
}

fn restore_node(conn: &Connection, node: &Bookmark, parent: i64, index: Option<i64>) -> Result<i64> {
    let id = insert_full(
        conn,
        parent,
        node.kind,
        &node.title,
        node.url.as_deref(),
        index,
        &node.guid,
        node.added_us,
        node.modified_us,
    )?;
    for c in &node.children {
        restore_node(conn, c, id, None)?;
    }
    Ok(id)
}

/// Firefox import. Idempotent by GUID; Firefox's root GUIDs map onto ours.
/// Returns the number of items inserted.
pub fn import_items(conn: &mut Connection, items: &[ImportedBookmark]) -> Result<usize> {
    let tx = conn.transaction()?;
    let mut inserted = 0;
    for item in items {
        if is_root_guid(&item.guid) || id_for_guid(&tx, &item.guid)?.is_some() {
            continue;
        }
        let Some(parent) = id_for_guid(&tx, &item.parent_guid)? else {
            continue; // parent was skipped (tags, livemarks)
        };
        insert_full(
            &tx,
            parent,
            item.kind,
            &item.title,
            item.url.as_deref(),
            None,
            &item.guid,
            item.added_us,
            item.modified_us,
        )?;
        inserted += 1;
    }
    tx.commit()?;
    Ok(inserted)
}

// --- Netscape bookmark HTML ------------------------------------------------

fn escape_html(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;")
}

fn unescape_html(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(i) = rest.find('&') {
        out.push_str(&rest[..i]);
        rest = &rest[i..];
        let Some(end) = rest.find(';').filter(|&e| e <= 10) else {
            out.push('&');
            rest = &rest[1..];
            continue;
        };
        let entity = &rest[1..end];
        let decoded = match entity {
            "amp" => Some('&'),
            "lt" => Some('<'),
            "gt" => Some('>'),
            "quot" => Some('"'),
            "apos" => Some('\''),
            "nbsp" => Some('\u{a0}'),
            _ => entity
                .strip_prefix("#x")
                .or_else(|| entity.strip_prefix("#X"))
                .and_then(|h| u32::from_str_radix(h, 16).ok())
                .or_else(|| entity.strip_prefix('#').and_then(|d| d.parse().ok()))
                .and_then(char::from_u32),
        };
        match decoded {
            Some(c) => {
                out.push(c);
                rest = &rest[end + 1..];
            }
            None => {
                out.push('&');
                rest = &rest[1..];
            }
        }
    }
    out.push_str(rest);
    out
}

/// Exports all bookmarks as Netscape bookmark HTML (readable by Chrome, Edge, Firefox).
pub fn export_html(conn: &Connection) -> Result<String> {
    let mut out = String::from(
        "<!DOCTYPE NETSCAPE-Bookmark-file-1>\n<!-- This is an automatically generated file.\n     It will be read and overwritten.\n     DO NOT EDIT! -->\n<META HTTP-EQUIV=\"Content-Type\" CONTENT=\"text/html; charset=UTF-8\">\n<TITLE>Bookmarks</TITLE>\n<H1>Bookmarks</H1>\n<DL><p>\n",
    );
    for (guid, _) in [ROOT_TITLES[1], ROOT_TITLES[0], ROOT_TITLES[2], ROOT_TITLES[3]] {
        let node = tree(conn, root_id(conn, guid)?)?;
        if node.children.is_empty() && guid != roots::TOOLBAR {
            continue;
        }
        write_folder(&mut out, &node, 1, guid == roots::TOOLBAR);
    }
    out.push_str("</DL><p>\n");
    Ok(out)
}

fn write_folder(out: &mut String, node: &Bookmark, depth: usize, toolbar: bool) {
    let pad = "    ".repeat(depth);
    let extra = if toolbar { " PERSONAL_TOOLBAR_FOLDER=\"true\"" } else { "" };
    out.push_str(&format!(
        "{pad}<DT><H3 ADD_DATE=\"{}\" LAST_MODIFIED=\"{}\"{extra}>{}</H3>\n{pad}<DL><p>\n",
        node.added_us / 1_000_000,
        node.modified_us / 1_000_000,
        escape_html(&node.title)
    ));
    for c in &node.children {
        match c.kind {
            BookmarkKind::Folder => write_folder(out, c, depth + 1, false),
            BookmarkKind::Separator => out.push_str(&format!("{pad}    <HR>\n")),
            BookmarkKind::Url => out.push_str(&format!(
                "{pad}    <DT><A HREF=\"{}\" ADD_DATE=\"{}\" LAST_MODIFIED=\"{}\">{}</A>\n",
                escape_html(c.url.as_deref().unwrap_or("")),
                c.added_us / 1_000_000,
                c.modified_us / 1_000_000,
                escape_html(&c.title)
            )),
        }
    }
    out.push_str(&format!("{pad}</DL><p>\n"));
}

struct Tag<'a> {
    name: String,
    closing: bool,
    attrs: &'a str,
}

fn attr(attrs: &str, name: &str) -> Option<String> {
    let lower = attrs.to_ascii_lowercase();
    let needle = format!("{}=", name.to_ascii_lowercase());
    let mut search_from = 0;
    while let Some(i) = lower[search_from..].find(&needle) {
        let at = search_from + i;
        let boundary = at == 0 || lower.as_bytes()[at - 1].is_ascii_whitespace();
        let value_start = at + needle.len();
        if boundary {
            let rest = &attrs[value_start..];
            let value = if let Some(r) = rest.strip_prefix('"') {
                &r[..r.find('"').unwrap_or(r.len())]
            } else if let Some(r) = rest.strip_prefix('\'') {
                &r[..r.find('\'').unwrap_or(r.len())]
            } else {
                &rest[..rest.find(char::is_whitespace).unwrap_or(rest.len())]
            };
            return Some(unescape_html(value));
        }
        search_from = value_start;
    }
    None
}

fn next_tag(s: &str) -> Option<(usize, usize, Tag<'_>)> {
    let start = s.find('<')?;
    let end = start + s[start..].find('>')?;
    let inner = &s[start + 1..end];
    let closing = inner.starts_with('/');
    let inner = inner.trim_start_matches('/');
    let name_end = inner.find(|c: char| c.is_whitespace()).unwrap_or(inner.len());
    Some((start, end + 1, Tag { name: inner[..name_end].to_ascii_uppercase(), closing, attrs: &inner[name_end..] }))
}

/// Imports Netscape bookmark HTML under `parent_id`. The exported "bookmarks bar"
/// folder (PERSONAL_TOOLBAR_FOLDER) merges into Limbo's bookmarks bar.
/// Returns the number of bookmarks and folders created.
pub fn import_html(conn: &mut Connection, html: &str, parent_id: i64) -> Result<usize> {
    let tx = conn.transaction()?;
    let toolbar = root_id(&tx, roots::TOOLBAR)?;
    let mut stack: Vec<i64> = Vec::new();
    let mut pending_folder: Option<i64> = None;
    let mut created = 0usize;
    let mut rest = html;
    while let Some((start, end, tag)) = next_tag(rest) {
        let after = &rest[end..];
        match (tag.name.as_str(), tag.closing) {
            ("DL", false) => {
                let target = pending_folder.take().unwrap_or(*stack.last().unwrap_or(&parent_id));
                stack.push(target);
            }
            ("DL", true) => {
                stack.pop();
            }
            ("H3", false) => {
                let close = after.to_ascii_uppercase().find("</H3>").unwrap_or(after.len());
                let title = unescape_html(after[..close].trim());
                let current = *stack.last().unwrap_or(&parent_id);
                let is_toolbar = attr(tag.attrs, "PERSONAL_TOOLBAR_FOLDER").is_some_and(|v| v.eq_ignore_ascii_case("true"));
                pending_folder = Some(if is_toolbar {
                    toolbar
                } else {
                    created += 1;
                    insert(&tx, current, BookmarkKind::Folder, &title, None, None)?
                });
                rest = &after[close..];
                continue;
            }
            ("A", false) => {
                let close = after.to_ascii_uppercase().find("</A>").unwrap_or(after.len());
                let title = unescape_html(after[..close].trim());
                if let Some(href) = attr(tag.attrs, "HREF").filter(|h| !h.is_empty() && !h.starts_with("javascript:")) {
                    let current = *stack.last().unwrap_or(&parent_id);
                    let added = attr(tag.attrs, "ADD_DATE").and_then(|d| d.parse::<i64>().ok());
                    let now = crate::db::now_us();
                    let added_us = added.map(|s| s * 1_000_000).unwrap_or(now);
                    insert_full(&tx, current, BookmarkKind::Url, &title, Some(&href), None, &new_guid(), added_us, added_us)?;
                    created += 1;
                }
                rest = &after[close..];
                continue;
            }
            ("HR", false) => {
                let current = *stack.last().unwrap_or(&parent_id);
                insert(&tx, current, BookmarkKind::Separator, "", None, None)?;
            }
            _ => {}
        }
        let _ = start;
        rest = after;
    }
    tx.commit()?;
    Ok(created)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::Db;

    fn setup() -> (Db, i64, i64) {
        let db = Db::open_in_memory().unwrap();
        let toolbar = root_id(db.conn(), roots::TOOLBAR).unwrap();
        let other = root_id(db.conn(), roots::OTHER).unwrap();
        (db, toolbar, other)
    }

    fn titles(conn: &Connection, parent: i64) -> Vec<String> {
        children(conn, parent).unwrap().into_iter().map(|b| b.title).collect()
    }

    #[test]
    fn insert_positions_and_star() {
        let (db, toolbar, _) = setup();
        let c = db.conn();
        insert(c, toolbar, BookmarkKind::Url, "B", Some("https://b.com/"), None).unwrap();
        insert(c, toolbar, BookmarkKind::Url, "A", Some("https://a.com/"), Some(0)).unwrap();
        insert(c, toolbar, BookmarkKind::Url, "C", Some("https://c.com/"), Some(99)).unwrap();
        assert_eq!(titles(c, toolbar), vec!["A", "B", "C"]);
        assert_eq!(find_by_url(c, "https://b.com/").unwrap().len(), 1);
        // Bookmarking creates a searchable place with a positive frecency.
        let m = history::search(c, "b.com", 5).unwrap();
        assert_eq!(m.len(), 1);
        assert!(m[0].bookmarked);
        assert!(insert(c, toolbar, BookmarkKind::Url, "no url", None, None).is_err());
    }

    #[test]
    fn move_remove_restore() {
        let (mut db, toolbar, other) = setup();
        let c = db.conn_mut();
        let folder = insert(c, toolbar, BookmarkKind::Folder, "Dev", None, None).unwrap();
        let rust = insert(c, folder, BookmarkKind::Url, "Rust", Some("https://rust-lang.org/"), None).unwrap();
        insert(c, folder, BookmarkKind::Url, "Docs", Some("https://docs.rs/"), None).unwrap();
        insert(c, toolbar, BookmarkKind::Url, "News", Some("https://news.ycombinator.com/"), None).unwrap();

        move_to(c, rust, other, None).unwrap();
        assert_eq!(titles(c, folder), vec!["Docs"]);
        assert_eq!(titles(c, other), vec!["Rust"]);
        assert!(move_to(c, folder, folder, None).is_err(), "no cycles");
        let root_toolbar = root_id(c, roots::TOOLBAR).unwrap();
        assert!(remove(c, root_toolbar).is_err());

        let removed = remove(c, folder).unwrap();
        assert_eq!(removed.children.len(), 1);
        assert_eq!(titles(c, toolbar), vec!["News"]);
        restore(c, &removed).unwrap();
        assert_eq!(titles(c, toolbar), vec!["Dev", "News"]);
        let dev = &children(c, toolbar).unwrap()[0];
        assert_eq!(dev.guid, removed.guid);
        assert_eq!(titles(c, dev.id), vec!["Docs"]);
    }

    #[test]
    fn html_roundtrip() {
        let (mut db, toolbar, other) = setup();
        {
            let c = db.conn_mut();
            let f = insert(c, toolbar, BookmarkKind::Folder, "Work & Play", None, None).unwrap();
            insert(c, f, BookmarkKind::Url, "Q&A <site>", Some("https://example.com/?a=1&b=\"2\""), None).unwrap();
            insert(c, toolbar, BookmarkKind::Separator, "", None, None).unwrap();
            insert(c, other, BookmarkKind::Url, "Other", Some("https://other.com/"), None).unwrap();
        }
        let html = export_html(db.conn()).unwrap();
        assert!(html.contains("PERSONAL_TOOLBAR_FOLDER=\"true\""));
        assert!(html.contains("Q&amp;A &lt;site&gt;"));

        let (mut db2, toolbar2, other2) = setup();
        let n = import_html(db2.conn_mut(), &html, other2).unwrap();
        let c2 = db2.conn();
        let bar = tree(c2, toolbar2).unwrap();
        assert_eq!(bar.children.len(), 2);
        assert_eq!(bar.children[0].title, "Work & Play");
        assert_eq!(bar.children[0].children[0].title, "Q&A <site>");
        assert_eq!(bar.children[0].children[0].url.as_deref(), Some("https://example.com/?a=1&b=\"2\""));
        assert_eq!(bar.children[1].kind, BookmarkKind::Separator);
        // "Other bookmarks" imports as a folder under the chosen parent.
        let other_tree = tree(c2, other2).unwrap();
        assert_eq!(other_tree.children[0].title, "Other bookmarks");
        assert_eq!(other_tree.children[0].children[0].title, "Other");
        assert_eq!(n, 4);
    }

    #[test]
    fn imports_chrome_style_html() {
        let html = r#"<!DOCTYPE NETSCAPE-Bookmark-file-1>
<DL><p>
    <DT><H3 ADD_DATE="1600000000" PERSONAL_TOOLBAR_FOLDER="true">Bookmarks bar</H3>
    <DL><p>
        <DT><A HREF="https://mail.google.com/" ADD_DATE="1600000001" ICON="data:image/png;base64,AAA">Gmail</A>
        <DT><A HREF="javascript:alert(1)">Bookmarklet</A>
        <DT><H3>Folder</H3>
        <DL><p>
            <DT><A HREF='https://x.com/'>X &#38; Y &#x263A;</A>
        </DL><p>
    </DL><p>
</DL><p>"#;
        let (mut db, toolbar, other) = setup();
        import_html(db.conn_mut(), html, other).unwrap();
        let bar = tree(db.conn(), toolbar).unwrap();
        assert_eq!(bar.children[0].title, "Gmail");
        assert_eq!(bar.children[0].added_us, 1_600_000_001_000_000);
        assert_eq!(bar.children[1].title, "Folder", "bookmarklets are skipped");
        assert_eq!(bar.children[1].children[0].title, "X & Y \u{263A}");
    }

    #[test]
    fn firefox_import_by_guid() {
        let (mut db, toolbar, _) = setup();
        let items = vec![
            ImportedBookmark {
                guid: "folderguid01".into(),
                parent_guid: roots::TOOLBAR.into(),
                kind: BookmarkKind::Folder,
                title: "Mozilla".into(),
                url: None,
                added_us: 1,
                modified_us: 2,
            },
            ImportedBookmark {
                guid: "bookmark0001".into(),
                parent_guid: "folderguid01".into(),
                kind: BookmarkKind::Url,
                title: "MDN".into(),
                url: Some("https://developer.mozilla.org/".into()),
                added_us: 3,
                modified_us: 4,
            },
            ImportedBookmark {
                guid: "orphan000001".into(),
                parent_guid: "tags________".into(),
                kind: BookmarkKind::Folder,
                title: "tag".into(),
                url: None,
                added_us: 0,
                modified_us: 0,
            },
        ];
        assert_eq!(import_items(db.conn_mut(), &items).unwrap(), 2);
        assert_eq!(import_items(db.conn_mut(), &items).unwrap(), 0, "idempotent");
        let bar = tree(db.conn(), toolbar).unwrap();
        assert_eq!(bar.children[0].children[0].title, "MDN");
        assert_eq!(bar.children[0].children[0].added_us, 3);
    }

    #[test]
    fn guids_are_unique_and_12_chars() {
        let a = new_guid();
        assert_eq!(a.len(), 12);
        assert_ne!(a, new_guid());
    }
}
