//! `manifest.json`: what the install dialog, toolbar and compatibility checks need.

use std::path::Path;

use serde::Serialize;
use serde_json::Value;

use crate::error::{Error, Result};

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Background {
    None,
    /// MV3 service worker (Chromium stops it when idle: cheap on RAM).
    ServiceWorker,
    /// `background.scripts` (MV2, or Firefox's MV3 event pages).
    Scripts,
    /// MV2 background page.
    Page,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Compatibility {
    Ok,
    /// Manifest V2: Chromium is removing support; may stop working.
    Mv2Deprecated,
    /// MV3 with only `background.scripts`: a Firefox-only add-on.
    FirefoxOnly,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ManifestInfo {
    pub name: String,
    pub version: String,
    pub manifest_version: u32,
    pub description: String,
    pub background: Background,
    /// Has an `action` / `browser_action` / `page_action` (gets a toolbar icon).
    pub has_action: bool,
    pub popup: Option<String>,
    pub action_title: Option<String>,
    /// `(size, path)` sorted by size.
    pub icons: Vec<(u32, String)>,
    pub action_icons: Vec<(u32, String)>,
    pub options_page: Option<String>,
    pub permissions: Vec<String>,
    pub host_permissions: Vec<String>,
    pub gecko_id: Option<String>,
}

/// Chrome accepts `//` and `/* */` comments in manifest.json.
pub fn strip_json_comments(src: &str) -> String {
    let mut out = String::with_capacity(src.len());
    let mut chars = src.chars().peekable();
    let mut in_str = false;
    while let Some(c) = chars.next() {
        if in_str {
            out.push(c);
            if c == '\\' {
                if let Some(n) = chars.next() {
                    out.push(n);
                }
            } else if c == '"' {
                in_str = false;
            }
            continue;
        }
        match (c, chars.peek()) {
            ('"', _) => {
                in_str = true;
                out.push(c);
            }
            ('/', Some('/')) => {
                for n in chars.by_ref() {
                    if n == '\n' {
                        out.push('\n');
                        break;
                    }
                }
            }
            ('/', Some('*')) => {
                chars.next();
                let mut prev = ' ';
                for n in chars.by_ref() {
                    if prev == '*' && n == '/' {
                        break;
                    }
                    prev = n;
                }
                out.push(' ');
            }
            _ => out.push(c),
        }
    }
    out
}

fn icons_of(v: Option<&Value>) -> Vec<(u32, String)> {
    let mut out: Vec<(u32, String)> = match v {
        Some(Value::String(s)) => vec![(0, s.clone())],
        Some(Value::Object(m)) => m
            .iter()
            .filter_map(|(k, v)| Some((k.parse().ok()?, v.as_str()?.to_string())))
            .collect(),
        _ => Vec::new(),
    };
    out.sort();
    out
}

fn strings(v: Option<&Value>) -> Vec<String> {
    v.and_then(Value::as_array)
        .map(|a| a.iter().filter_map(|x| x.as_str().map(str::to_string)).collect())
        .unwrap_or_default()
}

/// Resolves `__MSG_key__` placeholders from `_locales/<locale>/messages.json`.
pub fn localize(s: &str, messages: Option<&Value>) -> String {
    let Some(key) = s.strip_prefix("__MSG_").and_then(|k| k.strip_suffix("__")) else {
        return s.to_string();
    };
    let Some(Value::Object(m)) = messages else { return s.to_string() };
    m.iter()
        .find(|(k, _)| k.eq_ignore_ascii_case(key))
        .and_then(|(_, v)| v.get("message").and_then(Value::as_str))
        .map(str::to_string)
        .unwrap_or_else(|| s.to_string())
}

pub fn parse(json: &str, messages: Option<&Value>) -> Result<ManifestInfo> {
    let v: Value = serde_json::from_str(&strip_json_comments(json))?;
    let s = |k: &str| v.get(k).and_then(Value::as_str);
    let manifest_version = v.get("manifest_version").and_then(Value::as_u64).unwrap_or(2) as u32;
    let bg = v.get("background");
    let background = match bg {
        Some(b) if b.get("service_worker").is_some() => Background::ServiceWorker,
        Some(b) if b.get("scripts").is_some() => Background::Scripts,
        Some(b) if b.get("page").is_some() => Background::Page,
        _ => Background::None,
    };
    let action = v.get("action").or_else(|| v.get("browser_action")).or_else(|| v.get("page_action"));
    let options_page = s("options_page")
        .map(str::to_string)
        .or_else(|| v.get("options_ui").and_then(|o| o.get("page")).and_then(Value::as_str).map(str::to_string));
    let mut permissions = strings(v.get("permissions"));
    let mut host_permissions = strings(v.get("host_permissions"));
    if manifest_version < 3 {
        // MV2 lists hosts among permissions.
        let (hosts, perms): (Vec<String>, Vec<String>) =
            permissions.into_iter().partition(|p| p.contains("://") || p == "<all_urls>");
        permissions = perms;
        host_permissions.extend(hosts);
    }
    for cs in v.get("content_scripts").and_then(Value::as_array).into_iter().flatten() {
        for m in strings(cs.get("matches")) {
            if !host_permissions.contains(&m) {
                host_permissions.push(m);
            }
        }
    }
    let name = s("name").ok_or_else(|| Error::Format("manifest has no name".into()))?;
    Ok(ManifestInfo {
        name: localize(name, messages),
        version: s("version").ok_or_else(|| Error::Format("manifest has no version".into()))?.to_string(),
        manifest_version,
        description: localize(s("description").unwrap_or(""), messages),
        background,
        has_action: action.is_some(),
        popup: action.and_then(|a| a.get("default_popup")).and_then(Value::as_str).filter(|p| !p.is_empty()).map(str::to_string),
        action_title: action.and_then(|a| a.get("default_title")).and_then(Value::as_str).map(|t| localize(t, messages)),
        icons: icons_of(v.get("icons")),
        action_icons: icons_of(action.and_then(|a| a.get("default_icon"))),
        options_page,
        permissions,
        host_permissions,
        gecko_id: v
            .get("browser_specific_settings")
            .or_else(|| v.get("applications"))
            .and_then(|b| b.get("gecko"))
            .and_then(|g| g.get("id"))
            .and_then(Value::as_str)
            .map(str::to_string),
    })
}

fn default_locale(json: &str) -> Option<String> {
    serde_json::from_str::<Value>(&strip_json_comments(json))
        .ok()?
        .get("default_locale")
        .and_then(Value::as_str)
        .map(str::to_string)
}

/// Reads an unpacked extension folder.
pub fn read_dir(dir: &Path) -> Result<ManifestInfo> {
    let json = std::fs::read_to_string(dir.join("manifest.json"))
        .map_err(|_| Error::Format("no manifest.json in this folder".into()))?;
    let messages = default_locale(&json).and_then(|loc| {
        let text = std::fs::read_to_string(dir.join("_locales").join(&loc).join("messages.json")).ok()?;
        serde_json::from_str::<Value>(&strip_json_comments(&text)).ok()
    });
    parse(&json, messages.as_ref())
}

/// Reads the manifest from a ZIP archive (CRX payload, .zip or .xpi).
pub fn read_zip(archive: &[u8]) -> Result<ManifestInfo> {
    use std::io::Read;
    let mut zip = zip::ZipArchive::new(std::io::Cursor::new(archive))?;
    let mut read = |name: &str| -> Option<String> {
        let mut f = zip.by_name(name).ok()?;
        let mut s = String::new();
        f.read_to_string(&mut s).ok()?;
        Some(s)
    };
    let json = read("manifest.json").ok_or_else(|| Error::Format("the package has no manifest.json".into()))?;
    let messages = default_locale(&json)
        .and_then(|loc| read(&format!("_locales/{loc}/messages.json")))
        .and_then(|t| serde_json::from_str::<Value>(&strip_json_comments(&t)).ok());
    parse(&json, messages.as_ref())
}

impl ManifestInfo {
    pub fn compatibility(&self) -> Compatibility {
        if self.manifest_version >= 3 && self.background == Background::Scripts {
            Compatibility::FirefoxOnly
        } else if self.manifest_version < 3 {
            Compatibility::Mv2Deprecated
        } else {
            Compatibility::Ok
        }
    }

    /// The icon closest to (and preferably not smaller than) `size` pixels.
    pub fn best_icon(&self, size: u32) -> Option<&str> {
        fn pick(list: &[(u32, String)], size: u32) -> Option<&str> {
            list.iter().find(|(s, _)| *s >= size).or_else(|| list.last()).map(|(_, p)| p.as_str())
        }
        pick(&self.action_icons, size).or_else(|| pick(&self.icons, size))
    }

    /// Human-readable warnings, like Chrome's install dialog.
    pub fn permission_warnings(&self) -> Vec<String> {
        let mut out: Vec<String> = Vec::new();
        let mut push = |s: String| {
            if !out.contains(&s) {
                out.push(s);
            }
        };
        let all_hosts = self.host_permissions.iter().any(|h| {
            h == "<all_urls>" || h.starts_with("*://*/") || h.starts_with("http://*/") || h.starts_with("https://*/")
        });
        if all_hosts {
            push("Read and change all your data on all websites".into());
        } else {
            let mut hosts: Vec<String> = self
                .host_permissions
                .iter()
                .filter_map(|h| h.split("://").nth(1).map(|r| r.split('/').next().unwrap_or(r).to_string()))
                .collect();
            hosts.sort();
            hosts.dedup();
            match hosts.len() {
                0 => {}
                1..=3 => push(format!("Read and change your data on {}", hosts.join(", "))),
                n => push(format!("Read and change your data on {n} websites")),
            }
        }
        for p in &self.permissions {
            let w = match p.as_str() {
                "tabs" | "webNavigation" | "declarativeNetRequestFeedback" => "Read your browsing history",
                "history" => "Read and change your browsing history",
                "bookmarks" => "Read and change your bookmarks",
                "downloads" => "Manage your downloads",
                "downloads.open" => "Open downloaded files",
                "clipboardRead" => "Read data you copy and paste",
                "clipboardWrite" => "Modify data you copy and paste",
                "nativeMessaging" => "Communicate with cooperating native applications",
                "notifications" => "Display notifications",
                "geolocation" => "Detect your physical location",
                "management" => "Manage your apps, extensions, and themes",
                "privacy" => "Change your privacy-related settings",
                "proxy" => "Read and change all your data on all websites",
                "debugger" => "Access the page debugger backend",
                "declarativeNetRequest" | "declarativeNetRequestWithHostAccess" => "Block content on any page",
                "topSites" => "Read a list of your most frequently visited websites",
                "cookies" => "Read and change cookies",
                "webRequest" | "webRequestBlocking" if all_hosts => continue,
                "pageCapture" => "Read and change all your data on all websites",
                "desktopCapture" => "Capture content of your screen",
                "tabCapture" => "Capture content of your tabs",
                "sessions" => "Read your recently closed tabs",
                "contentSettings" => "Change your settings that control websites' access to features",
                _ => continue,
            };
            push(w.to_string());
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const UBOL: &str = r#"{
      // comment allowed
      "manifest_version": 3,
      "name": "__MSG_extName__",
      "version": "2025.1.1",
      "default_locale": "en",
      "background": { "service_worker": "/js/background.js", "type": "module" },
      "action": { "default_popup": "popup.html", "default_icon": { "16": "img/16.png", "32": "img/32.png", "64": "img/64.png" } },
      "icons": { "128": "img/128.png", "16": "img/16.png" },
      "permissions": ["declarativeNetRequest", "scripting", "storage", "activeTab"],
      "host_permissions": ["<all_urls>"],
      "options_ui": { "page": "dashboard.html" },
      "description": "An \"efficient\" /* not a comment */ blocker"
    }"#;

    #[test]
    fn parses_mv3() {
        let messages = serde_json::json!({"extName": {"message": "uBlock Origin Lite"}});
        let m = parse(UBOL, Some(&messages)).unwrap();
        assert_eq!(m.name, "uBlock Origin Lite");
        assert_eq!(m.description, "An \"efficient\" /* not a comment */ blocker");
        assert_eq!(m.background, Background::ServiceWorker);
        assert_eq!(m.popup.as_deref(), Some("popup.html"));
        assert_eq!(m.options_page.as_deref(), Some("dashboard.html"));
        assert_eq!(m.compatibility(), Compatibility::Ok);
        assert_eq!(m.best_icon(24), Some("img/32.png"));
        assert_eq!(m.best_icon(200), Some("img/64.png"));
        assert_eq!(
            m.permission_warnings(),
            vec!["Read and change all your data on all websites", "Block content on any page"]
        );
    }

    #[test]
    fn firefox_only_and_mv2() {
        let ff = r#"{"manifest_version":3,"name":"X","version":"1","background":{"scripts":["bg.js"]},
                    "browser_specific_settings":{"gecko":{"id":"x@y"}}}"#;
        let m = parse(ff, None).unwrap();
        assert_eq!(m.compatibility(), Compatibility::FirefoxOnly);
        assert_eq!(m.gecko_id.as_deref(), Some("x@y"));

        let mv2 = r#"{"manifest_version":2,"name":"Old","version":"1","background":{"scripts":["bg.js"]},
                     "permissions":["tabs","https://mail.google.com/*","storage"],
                     "browser_action":{"default_icon":"icon.png"},
                     "content_scripts":[{"matches":["https://docs.google.com/*"],"js":["c.js"]}]}"#;
        let m = parse(mv2, None).unwrap();
        assert_eq!(m.compatibility(), Compatibility::Mv2Deprecated);
        assert!(m.has_action);
        assert_eq!(m.popup, None);
        assert_eq!(m.host_permissions, vec!["https://mail.google.com/*", "https://docs.google.com/*"]);
        assert_eq!(
            m.permission_warnings(),
            vec!["Read and change your data on docs.google.com, mail.google.com", "Read your browsing history"]
        );
        assert_eq!(m.best_icon(16), Some("icon.png"));
    }

    #[test]
    fn missing_fields_error() {
        assert!(parse(r#"{"version":"1"}"#, None).is_err());
        assert!(parse(r#"{"name":"x"}"#, None).is_err());
        assert!(parse("not json", None).is_err());
    }

    #[test]
    fn reads_zip_with_locales() {
        let zip = super::super::crx::testutil::zip_with(&[
            ("manifest.json", r#"{"name":"__MSG_n__","version":"3.0","manifest_version":3,"default_locale":"en"}"#),
            ("_locales/en/messages.json", r#"{"N": {"message": "Localized"}}"#),
        ]);
        assert_eq!(read_zip(&zip).unwrap().name, "Localized");
    }
}
