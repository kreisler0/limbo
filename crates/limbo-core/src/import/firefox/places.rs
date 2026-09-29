//! History and bookmarks from `places.sqlite`.

use std::collections::HashMap;

use rusqlite::{Connection, params};

use crate::bookmarks::{BookmarkKind, ImportedBookmark};
use crate::db::roots;
use crate::error::Result;
use crate::frecency::Transition;
use crate::history::{self, ImportedPlace};

/// Reads history in batches of `batch` pages (keeps memory bounded on big
/// profiles) and hands each batch to `sink`. Returns the number of pages read.
pub fn read_history(
    conn: &Connection,
    batch: usize,
    mut sink: impl FnMut(Vec<ImportedPlace>, usize, usize) -> Result<()>,
) -> Result<usize> {
    let total: i64 = conn.query_row("SELECT COUNT(*) FROM moz_places", [], |r| r.get(0))?;
    let mut last_id = 0i64;
    let mut done = 0usize;
    let mut places_stmt = conn.prepare(
        "SELECT id, url, title, COALESCE(typed, 0) FROM moz_places WHERE id > ?1 ORDER BY id LIMIT ?2",
    )?;
    let mut visits_stmt = conn.prepare(
        "SELECT place_id, visit_date, visit_type FROM moz_historyvisits
         WHERE place_id BETWEEN ?1 AND ?2 AND visit_date IS NOT NULL",
    )?;
    loop {
        let rows: Vec<(i64, String, Option<String>, i64)> = places_stmt
            .query_map(params![last_id, batch as i64], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)))?
            .collect::<Result<_, _>>()?;
        if rows.is_empty() {
            break;
        }
        let first = rows[0].0;
        last_id = rows[rows.len() - 1].0;
        let mut by_id: HashMap<i64, ImportedPlace> = HashMap::with_capacity(rows.len());
        let mut order = Vec::with_capacity(rows.len());
        for (id, url, title, typed) in rows {
            if !history::is_recordable(&url) {
                continue;
            }
            order.push(id);
            by_id.insert(id, ImportedPlace { url, title, typed_count: typed, visits: Vec::new() });
        }
        let mut visits = visits_stmt.query(params![first, last_id])?;
        while let Some(r) = visits.next()? {
            let pid: i64 = r.get(0)?;
            if let Some(p) = by_id.get_mut(&pid) {
                p.visits.push((r.get(1)?, Transition::from_i64(r.get::<_, Option<i64>>(2)?.unwrap_or(1))));
            }
        }
        // Pages with no visits are bookmarks-only; the bookmark import brings them.
        let batch_places: Vec<ImportedPlace> =
            order.into_iter().filter_map(|id| by_id.remove(&id)).filter(|p| !p.visits.is_empty()).collect();
        done += batch_places.len();
        sink(batch_places, done, total as usize)?;
    }
    Ok(done)
}

/// Reads the bookmark tree (menu, toolbar, other, mobile) parents-first.
/// Tags, `place:` queries and legacy livemarks are skipped.
pub fn read_bookmarks(conn: &Connection) -> Result<Vec<ImportedBookmark>> {
    struct Row {
        id: i64,
        kind: i64,
        parent: i64,
        position: i64,
        title: String,
        added: i64,
        modified: i64,
        guid: String,
        url: Option<String>,
    }
    let mut stmt = conn.prepare(
        "SELECT b.id, b.type, COALESCE(b.parent, 0), COALESCE(b.position, 0), COALESCE(b.title, ''),
                COALESCE(b.dateAdded, 0), COALESCE(b.lastModified, 0), b.guid, p.url
         FROM moz_bookmarks b LEFT JOIN moz_places p ON p.id = b.fk",
    )?;
    let rows: Vec<Row> = stmt
        .query_map([], |r| {
            Ok(Row {
                id: r.get(0)?,
                kind: r.get(1)?,
                parent: r.get(2)?,
                position: r.get(3)?,
                title: r.get(4)?,
                added: r.get(5)?,
                modified: r.get(6)?,
                guid: r.get(7)?,
                url: r.get(8)?,
            })
        })?
        .collect::<Result<_, _>>()?;

    let mut children: HashMap<i64, Vec<usize>> = HashMap::new();
    for (i, r) in rows.iter().enumerate() {
        children.entry(r.parent).or_default().push(i);
    }
    for list in children.values_mut() {
        list.sort_by_key(|&i| rows[i].position);
    }
    let guid_of: HashMap<i64, &str> = rows.iter().map(|r| (r.id, r.guid.as_str())).collect();

    let mut out = Vec::new();
    let mut stack: Vec<usize> = Vec::new();
    for root in [roots::MENU, roots::TOOLBAR, roots::OTHER, roots::MOBILE] {
        let Some(root_row) = rows.iter().find(|r| r.guid == root) else { continue };
        if let Some(kids) = children.get(&root_row.id) {
            stack.extend(kids.iter().rev());
        }
        while let Some(i) = stack.pop() {
            let r = &rows[i];
            let kind = match r.kind {
                1 => BookmarkKind::Url,
                2 => BookmarkKind::Folder,
                3 => BookmarkKind::Separator,
                _ => continue,
            };
            if kind == BookmarkKind::Url {
                match &r.url {
                    Some(u) if !u.starts_with("place:") => {}
                    _ => continue,
                }
            }
            out.push(ImportedBookmark {
                guid: r.guid.clone(),
                parent_guid: guid_of.get(&r.parent).map(|g| g.to_string()).unwrap_or_default(),
                kind,
                title: r.title.clone(),
                url: if kind == BookmarkKind::Url { r.url.clone() } else { None },
                added_us: r.added,
                modified_us: r.modified.max(r.added),
            });
            if kind == BookmarkKind::Folder {
                if let Some(kids) = children.get(&r.id) {
                    stack.extend(kids.iter().rev());
                }
            }
        }
    }
    Ok(out)
}
