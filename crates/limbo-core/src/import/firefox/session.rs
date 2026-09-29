//! Open tabs from Firefox's session store (`*.jsonlz4`, "mozLz4" format:
//! magic `mozLz40\0`, u32 LE decompressed size, one LZ4 block).

use std::path::{Path, PathBuf};

use serde::Serialize;

use crate::error::{Error, Result};

pub const MAGIC: &[u8; 8] = b"mozLz40\0";

pub fn decode_mozlz4(data: &[u8]) -> Result<Vec<u8>> {
    let body = data.strip_prefix(&MAGIC[..]).ok_or_else(|| Error::Format("not a mozLz4 file".into()))?;
    if body.len() < 4 {
        return Err(Error::Format("truncated mozLz4 header".into()));
    }
    let size = u32::from_le_bytes([body[0], body[1], body[2], body[3]]) as usize;
    if size > 512 * 1024 * 1024 {
        return Err(Error::Format("mozLz4 size is implausible".into()));
    }
    lz4_flex::block::decompress(&body[4..], size).map_err(|e| Error::Format(format!("mozLz4: {e}")))
}

/// Encoder (tests and tooling).
pub fn encode_mozlz4(data: &[u8]) -> Vec<u8> {
    let mut out = MAGIC.to_vec();
    out.extend_from_slice(&(data.len() as u32).to_le_bytes());
    out.extend(lz4_flex::block::compress(data));
    out
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FirefoxTab {
    pub url: String,
    pub title: String,
    pub pinned: bool,
    pub favicon: Option<String>,
    /// The selected tab of the selected window.
    pub selected: bool,
}

/// The newest of the session files Firefox keeps.
pub fn session_file(profile: &Path) -> Option<PathBuf> {
    let candidates = [
        profile.join("sessionstore-backups").join("recovery.jsonlz4"),
        profile.join("sessionstore.jsonlz4"),
        profile.join("sessionstore-backups").join("previous.jsonlz4"),
    ];
    candidates
        .into_iter()
        .filter_map(|p| std::fs::metadata(&p).and_then(|m| m.modified()).ok().map(|t| (t, p)))
        .max_by_key(|(t, _)| *t)
        .map(|(_, p)| p)
}

fn keep_url(url: &str) -> bool {
    let lower = url.to_ascii_lowercase();
    lower.starts_with("http://") || lower.starts_with("https://") || lower.starts_with("file://")
}

/// Parses session JSON into tabs, in window order.
pub fn parse_session(json: &[u8]) -> Result<Vec<FirefoxTab>> {
    let v: serde_json::Value = serde_json::from_slice(json)?;
    let selected_window = v.get("selectedWindow").and_then(|s| s.as_u64()).unwrap_or(1) as usize;
    let mut out = Vec::new();
    let windows = v.get("windows").and_then(|w| w.as_array()).cloned().unwrap_or_default();
    for (wi, w) in windows.iter().enumerate() {
        let selected_tab = w.get("selected").and_then(|s| s.as_u64()).unwrap_or(1) as usize;
        let Some(tabs) = w.get("tabs").and_then(|t| t.as_array()) else { continue };
        for (ti, t) in tabs.iter().enumerate() {
            if t.get("hidden").and_then(|h| h.as_bool()) == Some(true) {
                continue;
            }
            let Some(entries) = t.get("entries").and_then(|e| e.as_array()) else { continue };
            if entries.is_empty() {
                continue;
            }
            let index = t.get("index").and_then(|i| i.as_u64()).unwrap_or(entries.len() as u64) as usize;
            let entry = &entries[index.clamp(1, entries.len()) - 1];
            let Some(url) = entry.get("url").and_then(|u| u.as_str()) else { continue };
            if !keep_url(url) {
                continue;
            }
            out.push(FirefoxTab {
                url: url.to_string(),
                title: entry.get("title").and_then(|x| x.as_str()).unwrap_or(url).to_string(),
                pinned: t.get("pinned").and_then(|p| p.as_bool()).unwrap_or(false),
                favicon: t.get("image").and_then(|i| i.as_str()).filter(|s| s.starts_with("http")).map(str::to_string),
                selected: wi + 1 == selected_window && ti + 1 == selected_tab,
            });
        }
    }
    if !out.iter().any(|t| t.selected) {
        if let Some(first) = out.first_mut() {
            first.selected = true;
        }
    }
    Ok(out)
}

pub fn read(profile: &Path) -> Result<Vec<FirefoxTab>> {
    let Some(path) = session_file(profile) else { return Ok(Vec::new()) };
    let data = std::fs::read(path)?;
    parse_session(&decode_mozlz4(&data)?)
}

#[cfg(test)]
mod tests {
    use super::*;

    const SESSION: &str = r#"{
      "version": ["sessionrestore", 1],
      "selectedWindow": 1,
      "windows": [{
        "selected": 2,
        "tabs": [
          {"entries": [{"url": "https://a.com/", "title": "A"}, {"url": "https://a.com/2", "title": "A2"}], "index": 2, "pinned": true, "image": "https://a.com/favicon.ico"},
          {"entries": [{"url": "https://b.com/", "title": "B"}], "index": 1},
          {"entries": [{"url": "about:newtab"}], "index": 1},
          {"entries": [{"url": "https://hidden.com/"}], "index": 1, "hidden": true},
          {"entries": [], "index": 0}
        ]
      }, {
        "selected": 1,
        "tabs": [{"entries": [{"url": "https://c.com/"}], "index": 1}]
      }]
    }"#;

    #[test]
    fn mozlz4_roundtrip() {
        let data = SESSION.as_bytes();
        let enc = encode_mozlz4(data);
        assert!(enc.starts_with(b"mozLz40\0"));
        assert_eq!(decode_mozlz4(&enc).unwrap(), data);
        assert!(decode_mozlz4(b"garbage!").is_err());
        assert!(decode_mozlz4(b"mozLz40\0\x01").is_err());
    }

    #[test]
    fn parses_tabs() {
        let tabs = parse_session(SESSION.as_bytes()).unwrap();
        let urls: Vec<&str> = tabs.iter().map(|t| t.url.as_str()).collect();
        assert_eq!(urls, vec!["https://a.com/2", "https://b.com/", "https://c.com/"]);
        assert!(tabs[0].pinned);
        assert_eq!(tabs[0].title, "A2");
        assert_eq!(tabs[0].favicon.as_deref(), Some("https://a.com/favicon.ico"));
        assert!(tabs[1].selected);
        assert_eq!(tabs[2].title, "https://c.com/");
    }

    #[test]
    fn picks_newest_file() {
        let dir = tempfile::tempdir().unwrap();
        assert!(read(dir.path()).unwrap().is_empty());
        std::fs::create_dir_all(dir.path().join("sessionstore-backups")).unwrap();
        std::fs::write(dir.path().join("sessionstore.jsonlz4"), encode_mozlz4(b"{\"windows\":[]}")).unwrap();
        std::thread::sleep(std::time::Duration::from_millis(20));
        std::fs::write(dir.path().join("sessionstore-backups/recovery.jsonlz4"), encode_mozlz4(SESSION.as_bytes())).unwrap();
        assert_eq!(read(dir.path()).unwrap().len(), 3);
    }
}
