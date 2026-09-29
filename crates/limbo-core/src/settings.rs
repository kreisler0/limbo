//! User settings (build plan 8.9), stored as key -> JSON value rows so new
//! settings get their defaults without a migration.

use serde::{Deserialize, Serialize};

use crate::lifecycle::Policy;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Theme {
    System,
    Light,
    Dark,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum TabLayout {
    /// Horizontal tab strip in the top bar (the owner's choice).
    Top,
    /// Vertical tabs in a pinned sidebar.
    Sidebar,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Settings {
    pub theme: Theme,
    /// Accent color override (`#RRGGBB`); `None` uses the design token.
    pub accent: Option<String>,
    /// Memory saver preset, timers and maximum awake tabs.
    pub memory: Policy,
    pub show_memory_in_toolbar: bool,
    pub show_google_suggestions: bool,
    pub restore_tabs_on_startup: bool,
    /// `None` = the user's Downloads folder.
    pub downloads_folder: Option<String>,
    pub ask_where_to_save: bool,
    pub bookmarks_bar_visible: bool,
    pub tab_layout: TabLayout,
    /// Default page zoom (1.0 = 100%).
    pub default_zoom: f64,
    pub developer_mode: bool,
    /// Extra Chromium flags; only applied when `experimental_args_enabled`.
    pub experimental_args: Vec<String>,
    pub experimental_args_enabled: bool,
    /// Microsoft Defender SmartScreen for web pages. The owner chose to turn it
    /// off to save memory (see docs/DECISIONS.md).
    pub smartscreen_enabled: bool,
    /// Windows 11 Mica backdrop behind the top bar.
    pub mica: bool,
    pub onboarding_done: bool,
    /// Offer to save passwords.
    pub offer_to_save_passwords: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            theme: Theme::System,
            accent: None,
            memory: Policy::balanced(),
            show_memory_in_toolbar: true,
            show_google_suggestions: true,
            restore_tabs_on_startup: true,
            downloads_folder: None,
            ask_where_to_save: false,
            bookmarks_bar_visible: false,
            tab_layout: TabLayout::Top,
            default_zoom: 1.0,
            developer_mode: false,
            experimental_args: Vec::new(),
            experimental_args_enabled: false,
            smartscreen_enabled: false,
            mica: true,
            onboarding_done: false,
            offer_to_save_passwords: true,
        }
    }
}

/// Onboarding default for "Open my Firefox tabs" (owner: yes).
pub const IMPORT_FIREFOX_TABS_BY_DEFAULT: bool = true;

impl Settings {
    /// Rebuilds settings from stored `(key, json)` rows. Unknown keys and values
    /// that fail to parse are ignored, falling back to defaults.
    pub fn from_rows<'a>(rows: impl IntoIterator<Item = (&'a str, &'a str)>) -> Settings {
        let mut obj = match serde_json::to_value(Settings::default()) {
            Ok(serde_json::Value::Object(m)) => m,
            _ => return Settings::default(),
        };
        for (key, json) in rows {
            if !obj.contains_key(key) {
                continue;
            }
            let Ok(value) = serde_json::from_str::<serde_json::Value>(json) else { continue };
            let previous = obj.insert(key.to_string(), value);
            // Validate this key alone so one bad row doesn't reset everything.
            if serde_json::from_value::<Settings>(serde_json::Value::Object(obj.clone())).is_err() {
                if let Some(prev) = previous {
                    obj.insert(key.to_string(), prev);
                }
            }
        }
        serde_json::from_value(serde_json::Value::Object(obj)).unwrap_or_default()
    }

    /// `(key, json)` rows for storage.
    pub fn to_rows(&self) -> Vec<(String, String)> {
        match serde_json::to_value(self) {
            Ok(serde_json::Value::Object(m)) => m.into_iter().map(|(k, v)| (k, v.to_string())).collect(),
            _ => Vec::new(),
        }
    }

    /// Applies a partial update (`{"theme": "dark"}`) coming from the UI.
    pub fn merged(&self, patch: &serde_json::Value) -> Result<Settings, serde_json::Error> {
        let mut base = serde_json::to_value(self)?;
        if let (serde_json::Value::Object(b), serde_json::Value::Object(p)) = (&mut base, patch) {
            for (k, v) in p {
                if b.contains_key(k) {
                    b.insert(k.clone(), v.clone());
                }
            }
        }
        serde_json::from_value(base)
    }

    /// Browser arguments for the shared WebView2 environment. Every webview must
    /// use exactly these, or WebView2 starts a second browser process.
    pub fn browser_args(&self) -> String {
        let mut disabled = vec!["msWebOOUI", "msPdfOOUI"];
        if !self.smartscreen_enabled {
            disabled.push("msSmartScreenProtection");
        }
        let mut args = format!("--disable-features={}", disabled.join(","));
        if self.experimental_args_enabled {
            for a in &self.experimental_args {
                let a = a.trim();
                // Only simple `--flag` / `--flag=value` tokens.
                if a.starts_with("--") && !a.contains(char::is_whitespace) && !a.starts_with("--disable-features") {
                    args.push(' ');
                    args.push_str(a);
                }
            }
        }
        args
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lifecycle::Preset;

    #[test]
    fn defaults_match_owner_answers() {
        let s = Settings::default();
        assert_eq!(s.tab_layout, TabLayout::Top);
        assert!(!s.smartscreen_enabled);
        assert!(s.show_memory_in_toolbar);
        assert_eq!(s.memory.max_awake, Some(4));
        const { assert!(IMPORT_FIREFOX_TABS_BY_DEFAULT) };
    }

    #[test]
    fn roundtrip_and_bad_rows() {
        let mut s = Settings::default();
        s.theme = Theme::Dark;
        s.memory = Policy::aggressive();
        let rows = s.to_rows();
        let back = Settings::from_rows(rows.iter().map(|(k, v)| (k.as_str(), v.as_str())));
        assert_eq!(back, s);

        let rows = [("theme", "\"dark\""), ("memory", "42"), ("unknown", "1"), ("mica", "not json")];
        let s = Settings::from_rows(rows);
        assert_eq!(s.theme, Theme::Dark);
        assert_eq!(s.memory, Policy::balanced(), "invalid value falls back to default");
        assert!(s.mica);
    }

    #[test]
    fn merge_patch() {
        let s = Settings::default();
        let patched = s.merged(&serde_json::json!({"theme": "light", "bogus": 1})).unwrap();
        assert_eq!(patched.theme, Theme::Light);
        assert!(s.merged(&serde_json::json!({"theme": "purple"})).is_err());
        let mem = s.merged(&serde_json::json!({"memory": Policy::off()})).unwrap();
        assert_eq!(mem.memory.preset, Preset::Off);
    }

    #[test]
    fn browser_args_are_stable() {
        let mut s = Settings::default();
        assert_eq!(s.browser_args(), "--disable-features=msWebOOUI,msPdfOOUI,msSmartScreenProtection");
        s.smartscreen_enabled = true;
        assert_eq!(s.browser_args(), "--disable-features=msWebOOUI,msPdfOOUI");
        s.experimental_args = vec!["--renderer-process-limit=4".into(), "--x y".into(), "bad".into()];
        assert_eq!(s.browser_args(), "--disable-features=msWebOOUI,msPdfOOUI");
        s.experimental_args_enabled = true;
        assert_eq!(s.browser_args(), "--disable-features=msWebOOUI,msPdfOOUI --renderer-process-limit=4");
    }
}
