//! Extension store URLs. The stores' own "Add" buttons can't talk to an
//! embedded WebView2, so Limbo shows its own "Add to Limbo" pill on detail pages.

use serde::{Deserialize, Serialize};
use url::Url;

use super::is_valid_id;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Store {
    ChromeWebStore,
    EdgeAddons,
}

impl Store {
    pub fn as_str(self) -> &'static str {
        match self {
            Store::ChromeWebStore => "chrome-web-store",
            Store::EdgeAddons => "edge-add-ons",
        }
    }

    pub fn parse(s: &str) -> Option<Store> {
        match s {
            "chrome-web-store" => Some(Store::ChromeWebStore),
            "edge-add-ons" => Some(Store::EdgeAddons),
            _ => None,
        }
    }

    /// Update-check endpoint (Omaha "update2" protocol).
    pub fn update_base(self) -> &'static str {
        match self {
            Store::ChromeWebStore => "https://clients2.google.com/service/update2/crx",
            Store::EdgeAddons => "https://edge.microsoft.com/extensionwebstorebase/v1/crx",
        }
    }

    /// Direct CRX download (redirects to the package).
    pub fn download_url(self, id: &str, prodversion: &str) -> String {
        match self {
            Store::ChromeWebStore => format!(
                "{}?response=redirect&prodversion={prodversion}&acceptformat=crx3&x=id%3D{id}%26installsource%3Dondemand%26uc",
                self.update_base()
            ),
            Store::EdgeAddons => {
                format!("{}?response=redirect&x=id%3D{id}%26installsource%3Dondemand%26uc", self.update_base())
            }
        }
    }
}

/// Recognizes an extension detail page, returning the store and extension ID.
pub fn parse_detail_url(url: &str) -> Option<(Store, String)> {
    let u = Url::parse(url).ok()?;
    let host = u.host_str()?;
    let segs: Vec<&str> = u.path_segments()?.filter(|s| !s.is_empty()).collect();
    let (store, rest): (Store, &[&str]) = match host {
        "chromewebstore.google.com" => (Store::ChromeWebStore, segs.strip_prefix(&["detail"])?),
        "chrome.google.com" => (Store::ChromeWebStore, segs.strip_prefix(&["webstore", "detail"])?),
        "microsoftedge.microsoft.com" => (Store::EdgeAddons, segs.strip_prefix(&["addons", "detail"])?),
        _ => return None,
    };
    rest.iter().rev().find(|s| is_valid_id(s)).map(|id| (store, id.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detail_urls() {
        let id = "ddkjiahejlhfcafbddmgiahcphecmpfh";
        assert_eq!(
            parse_detail_url(&format!("https://chromewebstore.google.com/detail/ublock-origin-lite/{id}?hl=en")),
            Some((Store::ChromeWebStore, id.into()))
        );
        assert_eq!(
            parse_detail_url(&format!("https://chromewebstore.google.com/detail/{id}")),
            Some((Store::ChromeWebStore, id.into()))
        );
        assert_eq!(
            parse_detail_url(&format!("https://chrome.google.com/webstore/detail/x/{id}")),
            Some((Store::ChromeWebStore, id.into()))
        );
        assert_eq!(
            parse_detail_url(&format!("https://microsoftedge.microsoft.com/addons/detail/ublock/{id}")),
            Some((Store::EdgeAddons, id.into()))
        );
        assert_eq!(parse_detail_url("https://chromewebstore.google.com/category/extensions"), None);
        assert_eq!(parse_detail_url(&format!("https://evil.com/detail/x/{id}")), None);
    }

    #[test]
    fn download_urls() {
        let u = Store::ChromeWebStore.download_url("abc", "140.0.0.0");
        assert!(
            u.starts_with("https://clients2.google.com/service/update2/crx?response=redirect&prodversion=140.0.0.0")
        );
        assert!(u.contains("x=id%3Dabc%26installsource%3Dondemand%26uc"));
        assert_eq!(Store::parse(Store::EdgeAddons.as_str()), Some(Store::EdgeAddons));
    }
}
