//! Firefox profile discovery (`profiles.ini` + `installs.ini`) and lock detection.

use std::path::{Path, PathBuf};

use serde::Serialize;

use crate::error::Result;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FirefoxProfile {
    pub name: String,
    pub path: PathBuf,
    /// The profile Firefox opens by default (installs.ini, else `Default=1`).
    pub is_default: bool,
    /// Firefox is running with this profile (its databases are locked).
    pub running: bool,
    pub has_passwords: bool,
    pub has_history: bool,
}

/// `%APPDATA%\Mozilla\Firefox` (or `~/.mozilla/firefox` elsewhere, for development).
pub fn default_root() -> Option<PathBuf> {
    if cfg!(windows) {
        std::env::var_os("APPDATA").map(|a| PathBuf::from(a).join("Mozilla").join("Firefox"))
    } else {
        std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".mozilla").join("firefox"))
    }
}

/// A parsed INI file: `(section, [(key, value)])` in file order.
pub fn parse_ini(text: &str) -> Vec<(String, Vec<(String, String)>)> {
    let mut out: Vec<(String, Vec<(String, String)>)> = Vec::new();
    for line in text.lines() {
        let line = line.trim().trim_start_matches('\u{feff}');
        if line.is_empty() || line.starts_with(';') || line.starts_with('#') {
            continue;
        }
        if let Some(name) = line.strip_prefix('[').and_then(|l| l.strip_suffix(']')) {
            out.push((name.trim().to_string(), Vec::new()));
        } else if let (Some((k, v)), Some(section)) = (line.split_once('='), out.last_mut()) {
            section.1.push((k.trim().to_string(), v.trim().to_string()));
        }
    }
    out
}

fn get<'a>(entries: &'a [(String, String)], key: &str) -> Option<&'a str> {
    entries.iter().find(|(k, _)| k.eq_ignore_ascii_case(key)).map(|(_, v)| v.as_str())
}

/// Profile paths in profiles.ini use `/` even on Windows.
fn resolve(root: &Path, path: &str, relative: bool) -> PathBuf {
    if relative {
        path.split(['/', '\\']).filter(|s| !s.is_empty()).fold(root.to_path_buf(), |p, s| p.join(s))
    } else {
        PathBuf::from(path)
    }
}

/// Lists profiles under a Firefox root, default first.
pub fn discover(root: &Path) -> Result<Vec<FirefoxProfile>> {
    let Ok(text) = std::fs::read_to_string(root.join("profiles.ini")) else {
        return Ok(Vec::new());
    };
    let ini = parse_ini(&text);
    let install_default: Option<PathBuf> = std::fs::read_to_string(root.join("installs.ini"))
        .ok()
        .map(|t| parse_ini(&t))
        .into_iter()
        .flatten()
        .chain(ini.iter().filter(|(s, _)| s.starts_with("Install")).cloned())
        .find_map(|(_, e)| get(&e, "Default").map(|d| resolve(root, d, true)));

    let mut profiles = Vec::new();
    for (section, entries) in &ini {
        if !section.starts_with("Profile") {
            continue;
        }
        let Some(path) = get(entries, "Path") else { continue };
        let relative = get(entries, "IsRelative").is_none_or(|v| v == "1");
        let path = resolve(root, path, relative);
        let name = get(entries, "Name").unwrap_or("Profile").to_string();
        let default_flag = get(entries, "Default") == Some("1");
        let is_default = match &install_default {
            Some(d) => same_path(d, &path),
            None => default_flag,
        };
        profiles.push(describe(name, path, is_default));
    }
    if !profiles.iter().any(|p| p.is_default)
        && let Some(p) = profiles.iter_mut().find(|p| p.has_history)
    {
        p.is_default = true;
    }
    profiles.sort_by_key(|p| !p.is_default);
    Ok(profiles)
}

fn same_path(a: &Path, b: &Path) -> bool {
    let norm = |p: &Path| p.to_string_lossy().replace('\\', "/").trim_end_matches('/').to_lowercase();
    norm(a) == norm(b)
}

/// "Choose folder…": describes an arbitrary profile directory.
pub fn from_dir(dir: &Path) -> FirefoxProfile {
    let name = dir.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| "Profile".into());
    describe(name, dir.to_path_buf(), false)
}

fn describe(name: String, path: PathBuf, is_default: bool) -> FirefoxProfile {
    FirefoxProfile {
        running: is_running(&path),
        has_passwords: path.join("logins.json").is_file() && path.join("key4.db").is_file(),
        has_history: path.join("places.sqlite").is_file(),
        name,
        path,
        is_default,
    }
}

/// True while Firefox holds the profile lock. On Windows `parent.lock` stays on
/// disk after Firefox exits, so the test is whether it can be opened for writing.
pub fn is_running(profile: &Path) -> bool {
    if cfg!(windows) {
        let lock = profile.join("parent.lock");
        lock.exists() && std::fs::OpenOptions::new().write(true).open(&lock).is_err()
    } else {
        std::fs::symlink_metadata(profile.join("lock")).is_ok()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ini_parsing() {
        let ini = parse_ini(
            "\u{feff}[General]\nStartWithLastProfile=1\n; comment\n[Profile0]\nName = default\nPath=Profiles/abc.default\n",
        );
        assert_eq!(ini.len(), 2);
        assert_eq!(ini[1].0, "Profile0");
        assert_eq!(get(&ini[1].1, "name"), Some("default"));
    }

    #[test]
    fn discovery_prefers_install_default() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        for p in ["Profiles/aaa.default", "Profiles/bbb.default-release"] {
            std::fs::create_dir_all(root.join(p)).unwrap();
            std::fs::write(root.join(p).join("places.sqlite"), b"").unwrap();
        }
        std::fs::write(
            root.join("profiles.ini"),
            "[Profile1]\nName=default\nIsRelative=1\nPath=Profiles/aaa.default\nDefault=1\n\n\
             [Profile0]\nName=default-release\nIsRelative=1\nPath=Profiles/bbb.default-release\n\n\
             [General]\nStartWithLastProfile=1\nVersion=2\n",
        )
        .unwrap();
        std::fs::write(
            root.join("installs.ini"),
            "[308046B0AF4A39CB]\nDefault=Profiles/bbb.default-release\nLocked=1\n",
        )
        .unwrap();
        let profiles = discover(root).unwrap();
        assert_eq!(profiles.len(), 2);
        assert_eq!(profiles[0].name, "default-release");
        assert!(profiles[0].is_default);
        assert!(!profiles[1].is_default);
        assert!(profiles[0].has_history);
        assert!(!profiles[0].has_passwords);
        assert!(!profiles[0].running);

        // Without installs.ini, Default=1 wins.
        std::fs::remove_file(root.join("installs.ini")).unwrap();
        let profiles = discover(root).unwrap();
        assert_eq!(profiles[0].name, "default");
    }

    #[test]
    fn missing_root_is_empty() {
        let dir = tempfile::tempdir().unwrap();
        assert!(discover(&dir.path().join("nope")).unwrap().is_empty());
    }
}
