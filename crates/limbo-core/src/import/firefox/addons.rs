//! Installed add-ons from `extensions.json`, mapped to Chrome Web Store equivalents.

use std::path::Path;

use serde::Serialize;

use crate::error::Result;
use crate::extensions::addon_map::{self, ChromeEquivalent};

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FirefoxAddon {
    pub id: String,
    pub name: String,
    pub active: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub chrome: Option<ChromeEquivalent>,
    pub firefox_only: bool,
    /// Where to look when there's no known equivalent.
    pub search_url: String,
}

/// User-installed extensions (not themes, dictionaries or built-in system add-ons).
pub fn parse(json: &str) -> Result<Vec<FirefoxAddon>> {
    let v: serde_json::Value = serde_json::from_str(json)?;
    let mut out = Vec::new();
    for a in v.get("addons").and_then(|a| a.as_array()).into_iter().flatten() {
        let s = |k: &str| a.get(k).and_then(|x| x.as_str());
        if s("type") != Some("extension") || s("location") != Some("app-profile") {
            continue;
        }
        if a.get("hidden").and_then(|h| h.as_bool()) == Some(true) {
            continue;
        }
        let Some(id) = s("id") else { continue };
        let name =
            a.get("defaultLocale").and_then(|l| l.get("name")).and_then(|n| n.as_str()).unwrap_or(id).to_string();
        let active = a.get("active").and_then(|x| x.as_bool()).unwrap_or(false);
        out.push(FirefoxAddon {
            chrome: addon_map::lookup(id),
            firefox_only: addon_map::is_firefox_only(id),
            search_url: addon_map::store_search_url(&name),
            id: id.to_string(),
            name,
            active,
        });
    }
    out.sort_by(|a, b| {
        b.chrome.is_some().cmp(&a.chrome.is_some()).then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
    });
    Ok(out)
}

pub fn read(profile: &Path) -> Result<Vec<FirefoxAddon>> {
    match std::fs::read_to_string(profile.join("extensions.json")) {
        Ok(text) => parse(&text),
        Err(_) => Ok(Vec::new()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn filters_and_maps() {
        let json = r#"{"schemaVersion":36,"addons":[
          {"id":"uBlock0@raymondhill.net","type":"extension","location":"app-profile","active":true,"defaultLocale":{"name":"uBlock Origin"}},
          {"id":"some@weird.addon","type":"extension","location":"app-profile","active":false,"defaultLocale":{"name":"Weird Thing"}},
          {"id":"@testpilot-containers","type":"extension","location":"app-profile","active":true,"defaultLocale":{"name":"Firefox Multi-Account Containers"}},
          {"id":"formautofill@mozilla.org","type":"extension","location":"app-builtin","active":true},
          {"id":"default-theme@mozilla.org","type":"theme","location":"app-builtin","active":true},
          {"id":"en-US@dictionaries","type":"dictionary","location":"app-profile","active":true}
        ]}"#;
        let addons = parse(json).unwrap();
        assert_eq!(addons.len(), 3);
        assert_eq!(addons[0].name, "uBlock Origin");
        assert_eq!(addons[0].chrome.unwrap().id, "ddkjiahejlhfcafbddmgiahcphecmpfh");
        assert!(addons[1].firefox_only);
        assert_eq!(addons[2].search_url, "https://chromewebstore.google.com/search/Weird%20Thing");
        assert!(!addons[2].active);
    }
}
