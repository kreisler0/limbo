//! Favicons from `favicons.sqlite` (Firefox 55+).

use std::collections::HashMap;

use rusqlite::Connection;

use crate::error::Result;

/// Firefox stores SVG icons with width = u16::MAX.
const SVG_WIDTH: i64 = 65535;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PageIcon {
    pub page_url: String,
    pub icon_url: String,
    pub width: u32,
    pub data: Vec<u8>,
}

/// Picks one icon per page: the largest <= 64 px, else an SVG, else the smallest.
pub fn read(conn: &Connection, mut wanted: impl FnMut(&str) -> bool) -> Result<Vec<PageIcon>> {
    let mut stmt = conn.prepare(
        "SELECT p.page_url, i.icon_url, i.width, i.data
         FROM moz_pages_w_icons p
         JOIN moz_icons_to_pages ip ON ip.page_id = p.id
         JOIN moz_icons i ON i.id = ip.icon_id
         WHERE i.data IS NOT NULL",
    )?;
    let mut best: HashMap<String, PageIcon> = HashMap::new();
    let rank = |w: i64| -> i64 {
        if w == SVG_WIDTH {
            1_000 // after any <= 64 bitmap
        } else if w <= 64 {
            10_000 + w
        } else {
            -w
        }
    };
    let mut rows = stmt.query([])?;
    while let Some(r) = rows.next()? {
        let page_url: String = r.get(0)?;
        if !wanted(&page_url) {
            continue;
        }
        let width: i64 = r.get(2)?;
        let replace = match best.get(&page_url) {
            Some(cur) => rank(width) > rank(if cur.width == SVG_WIDTH as u32 { SVG_WIDTH } else { cur.width as i64 }),
            None => true,
        };
        if replace {
            let icon = PageIcon {
                page_url: page_url.clone(),
                icon_url: r.get(1)?,
                width: width.clamp(0, SVG_WIDTH) as u32,
                data: r.get(3)?,
            };
            best.insert(page_url, icon);
        }
    }
    let mut out: Vec<PageIcon> = best.into_values().collect();
    out.sort_by(|a, b| a.page_url.cmp(&b.page_url));
    Ok(out)
}
