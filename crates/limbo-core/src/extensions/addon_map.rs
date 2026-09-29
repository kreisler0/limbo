//! Curated Firefox add-on -> Chrome Web Store equivalents, offered during import.
//! The user installs each one with a click; nothing is installed silently.
//!
//! [VERIFY] These IDs were compiled from memory while the store sites were not
//! reachable from the build environment. Each must be checked against
//! chromewebstore.google.com before release (docs/DECISIONS.md). A wrong ID
//! is contained: the install dialog shows the downloaded manifest's name and
//! permissions and the user confirms before anything is installed.

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ChromeEquivalent {
    pub id: &'static str,
    pub name: &'static str,
}

/// `(firefox add-on id, chrome web store id, chrome name)`
pub const MAP: &[(&str, &str, &str)] = &[
    // uBlock Origin's MV3 sibling: declarativeNetRequest only, almost no RAM.
    ("uBlock0@raymondhill.net", "ddkjiahejlhfcafbddmgiahcphecmpfh", "uBlock Origin Lite"),
    ("{446900e4-71c2-419f-a6a7-df9c091e268b}", "nngceckbapebfimnlniiiahkandclblb", "Bitwarden Password Manager"),
    ("addon@darkreader.org", "eimadpbcbfnmbkopoojfekhnkhdbieeh", "Dark Reader"),
    ("{d634138d-c276-4fc8-924b-40a0ea21d284}", "aeblfdkhhhdcdjpifhhbdiojplfjncoa", "1Password"),
    ("support@lastpass.com", "hdokiejnpimakedhajhdlcegeplioahd", "LastPass"),
    ("87677a2c52b84ad3a151a4a72f5bd3c4@jetpack", "kbfnbcaeplbcioakkpcpgfkobkghlhen", "Grammarly"),
    ("jid1-MnnxcxisBPnSXQ@jetpack", "pkehgijcmpdhfbdbbnkijodmdjhbjlgp", "Privacy Badger"),
    ("{762f9885-5a13-4abd-9c77-433dcd38b8fd}", "gebbhagfogifgggkldgodflihgfeippi", "Return YouTube Dislike"),
    ("sponsorBlocker@ajay.app", "mnjggcdmjocbbbhaepdhchncahnbgone", "SponsorBlock"),
    ("firefox@tampermonkey.net", "dhdgffkkebhmkfjojejmpbldmpobfkfo", "Tampermonkey"),
    ("{aecec67f-0d10-4fa7-b7c7-609a2db280cf}", "jinjaccalgkegednnccohejagnlnfdag", "Violentmonkey"),
    ("jid1-93CWPmRbVPjRQA@jetpack", "bmnlcjabgnpnenekpadlanbbkooimhnj", "Honey"),
    ("@react-devtools", "fmkadmapgofadopljbjfkapdkoienihi", "React Developer Tools"),
    ("{5caff8cc-3d2e-4110-a88a-003cc85b3858}", "nhdogjmejiglipccpnnnanhbledajbpd", "Vue.js devtools"),
    ("jid1-BoFifL9Vbdl2zQ@jetpack", "ldpochfccmkkmhdbclfhpagapcfdljkj", "Decentraleyes"),
    ("firefox@ghostery.com", "mlomiejdfkolichcflejclcbmpeaniij", "Ghostery"),
    ("{7a7a4a92-a2a0-41d1-9fd7-1e92480d612d}", "clngdbkpkpeebahjckkjfobafhncgmne", "Stylus"),
    ("78272b6fa58f4a1abaac99321d503a20@proton.me", "ghmbeldphafepmbegfdlkpapadhbakde", "Proton Pass"),
    ("{74145f27-f039-47ce-a470-a662b129930a}", "lckanjgmijmafbedllaakclkaicjfmnk", "ClearURLs"),
    ("enhancerforyoutube@maximerf.addons.mozilla.org", "ponfpcnoihfmfllpaingbgckeeldkhle", "Enhancer for YouTube"),
    ("jid1-93WyvpgvxzGATw@jetpack", "aapbdbdomjkkjkaonfhkkikfgjllcleb", "Google Translate"),
];

/// Firefox add-ons with no Chrome version (shown as "Firefox only").
pub const FIREFOX_ONLY: &[&str] = &["@testpilot-containers", "{c607c8df-14a7-4f28-894f-29e8722976af}"];

pub fn lookup(firefox_id: &str) -> Option<ChromeEquivalent> {
    MAP.iter().find(|(ff, _, _)| *ff == firefox_id).map(|&(_, id, name)| ChromeEquivalent { id, name })
}

pub fn is_firefox_only(firefox_id: &str) -> bool {
    FIREFOX_ONLY.contains(&firefox_id)
}

/// Chrome Web Store search page for unmapped add-ons.
pub fn store_search_url(name: &str) -> String {
    let q: String = percent_encoding::utf8_percent_encode(name, percent_encoding::NON_ALPHANUMERIC).collect();
    format!("https://chromewebstore.google.com/search/{q}")
}

/// Chrome Web Store detail page.
pub fn store_detail_url(id: &str) -> String {
    format!("https://chromewebstore.google.com/detail/{id}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_are_well_formed_and_unique() {
        for (ff, id, name) in MAP {
            assert!(super::super::is_valid_id(id), "{name}: {id}");
            assert_eq!(MAP.iter().filter(|(f, _, _)| f == ff).count(), 1, "{ff}");
        }
    }

    #[test]
    fn lookups() {
        assert_eq!(lookup("uBlock0@raymondhill.net").unwrap().name, "uBlock Origin Lite");
        assert!(lookup("unknown@addon").is_none());
        assert!(is_firefox_only("@testpilot-containers"));
        assert_eq!(store_search_url("Dark Reader"), "https://chromewebstore.google.com/search/Dark%20Reader");
    }
}
