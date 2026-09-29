//! Omnibox input classification: URL vs Google search vs internal page.
//!
//! Order of rules (see the build plan, section 8.1):
//! 1. A leading `?` forces a search (the `?` is stripped). A trailing `?` also forces one.
//! 2. An explicit, known scheme navigates (`http:`, `https:`, `file:`, `mailto:`, ...).
//!    `edge:`/`chrome:` are blocked except for pages Limbo implements itself.
//! 3. Something that looks like a host (public suffix, `localhost`, IP literal,
//!    optionally with port/path) navigates to `https://` (or `http://` for local hosts).
//! 4. Everything else searches Google.

use serde::Serialize;
use url::Url;

pub const GOOGLE_HOME: &str = "https://www.google.com/";
const GOOGLE_SEARCH: &str = "https://www.google.com/search?q=";

/// Pages rendered by Limbo's own UI instead of a web page.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum InternalPage {
    NewTab,
    Settings,
    History,
    Bookmarks,
    Downloads,
    Extensions,
    Passwords,
}

impl InternalPage {
    pub fn url(self) -> &'static str {
        match self {
            InternalPage::NewTab => "limbo://newtab",
            InternalPage::Settings => "limbo://settings",
            InternalPage::History => "limbo://history",
            InternalPage::Bookmarks => "limbo://bookmarks",
            InternalPage::Downloads => "limbo://downloads",
            InternalPage::Extensions => "limbo://extensions",
            InternalPage::Passwords => "limbo://passwords",
        }
    }

    pub fn title(self) -> &'static str {
        match self {
            InternalPage::NewTab => "New Tab",
            InternalPage::Settings => "Settings",
            InternalPage::History => "History",
            InternalPage::Bookmarks => "Bookmarks",
            InternalPage::Downloads => "Downloads",
            InternalPage::Extensions => "Extensions",
            InternalPage::Passwords => "Passwords",
        }
    }

    /// Parses `limbo://<page>` (and the Firefox/Chrome/Edge spellings people type from habit).
    pub fn from_url(url: &str) -> Option<InternalPage> {
        let lower = url.trim().to_ascii_lowercase();
        let (scheme, rest) = lower.split_once(':')?;
        let rest = rest.trim_start_matches('/');
        let page = rest.split(['/', '?', '#']).next().unwrap_or("");
        match scheme {
            "limbo" => match page {
                "" | "newtab" | "home" => Some(InternalPage::NewTab),
                "settings" | "preferences" => Some(InternalPage::Settings),
                "history" => Some(InternalPage::History),
                "bookmarks" => Some(InternalPage::Bookmarks),
                "downloads" => Some(InternalPage::Downloads),
                "extensions" | "addons" => Some(InternalPage::Extensions),
                "passwords" => Some(InternalPage::Passwords),
                _ => None,
            },
            "edge" | "chrome" => match page {
                "downloads" => Some(InternalPage::Downloads),
                "settings" => {
                    if rest.starts_with("settings/passwords") {
                        Some(InternalPage::Passwords)
                    } else {
                        Some(InternalPage::Settings)
                    }
                }
                "history" => Some(InternalPage::History),
                "bookmarks" | "favorites" => Some(InternalPage::Bookmarks),
                "extensions" => Some(InternalPage::Extensions),
                "newtab" => Some(InternalPage::NewTab),
                _ => None,
            },
            "about" => match page {
                "newtab" | "home" => Some(InternalPage::NewTab),
                "preferences" | "settings" => Some(InternalPage::Settings),
                "addons" => Some(InternalPage::Extensions),
                "downloads" => Some(InternalPage::Downloads),
                "logins" => Some(InternalPage::Passwords),
                "history" | "library" => Some(InternalPage::History),
                _ => None,
            },
            _ => None,
        }
    }
}

/// Where omnibox input should take the user.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum Destination {
    Navigate { url: String },
    Search { query: String, url: String },
    Internal { page: InternalPage, url: String },
    Blocked { reason: BlockReason },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum BlockReason {
    /// `javascript:` / `vbscript:` typed or pasted into the address bar.
    ScriptUrl,
    /// `edge://` / `chrome://` / `about:` engine pages outside developer mode.
    EnginePage,
}

#[derive(Debug, Clone, Copy, Default)]
pub struct ClassifyOptions {
    /// Allows `edge://` engine pages (process-internals etc.) for profiling.
    pub developer_mode: bool,
}

/// Builds the Google search URL for a query.
pub fn google_search_url(query: &str) -> String {
    let encoded: String = url::form_urlencoded::byte_serialize(query.as_bytes()).collect();
    format!("{GOOGLE_SEARCH}{encoded}")
}

/// Classifies omnibox input. Returns `None` for empty input.
pub fn classify(input: &str, opts: ClassifyOptions) -> Option<Destination> {
    let text = input.trim();
    if text.is_empty() {
        return None;
    }

    // Rule 1: forced search.
    if let Some(rest) = text.strip_prefix('?') {
        let query = rest.trim();
        if query.is_empty() {
            return None;
        }
        return Some(search(query));
    }
    if text.ends_with('?') {
        return Some(search(text));
    }

    // Rule 2: explicit scheme (or a Windows path).
    if let Some(dest) = classify_with_scheme(text, opts) {
        return Some(dest);
    }

    // Rule 3: looks like a host.
    if let Some(url) = host_like_url(text) {
        return Some(Destination::Navigate { url });
    }

    // Rule 4: search.
    Some(search(text))
}

fn search(query: &str) -> Destination {
    Destination::Search { query: query.to_string(), url: google_search_url(query) }
}

/// Ctrl+Enter: wraps a bare word as `https://www.<input>.com/`.
pub fn ctrl_enter_url(input: &str) -> Option<String> {
    let text = input.trim();
    if text.is_empty() || text.contains(char::is_whitespace) || text.contains(':') {
        return None;
    }
    let (host, rest) = match text.find(['/', '?', '#']) {
        Some(i) => (&text[..i], &text[i..]),
        None => (text, ""),
    };
    if host.is_empty() {
        return None;
    }
    let mut host = host.to_ascii_lowercase();
    if !host.contains('.') {
        host = format!("{host}.com");
    }
    if !host.starts_with("www.") {
        host = format!("www.{host}");
    }
    let rest = if rest.is_empty() { "/" } else { rest };
    Url::parse(&format!("https://{host}{rest}")).ok().map(String::from)
}

const NAVIGABLE_SCHEMES: &[&str] = &[
    "http",
    "https",
    "file",
    "data",
    "view-source",
    "chrome-extension",
    "blob",
    "ftp",
    "ws",
    "wss",
    "mailto",
    "tel",
    "sms",
    "magnet",
    "news",
    "irc",
    "ircs",
    "webcal",
    "geo",
];

fn classify_with_scheme(text: &str, opts: ClassifyOptions) -> Option<Destination> {
    // Windows drive paths: C:\foo or C:/foo
    let bytes = text.as_bytes();
    if bytes.len() >= 3 && bytes[0].is_ascii_alphabetic() && bytes[1] == b':' && (bytes[2] == b'\\' || bytes[2] == b'/')
    {
        let path = text.replace('\\', "/");
        return Url::parse(&format!("file:///{path}")).ok().map(|u| Destination::Navigate { url: u.into() });
    }
    // UNC paths: \\server\share
    if let Some(rest) = text.strip_prefix("\\\\") {
        let path = rest.replace('\\', "/");
        return Url::parse(&format!("file://{path}")).ok().map(|u| Destination::Navigate { url: u.into() });
    }

    let colon = text.find(':')?;
    let scheme = &text[..colon];
    if scheme.is_empty()
        || !scheme.as_bytes()[0].is_ascii_alphabetic()
        || !scheme.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'+' || b == b'-' || b == b'.')
    {
        return None;
    }
    let rest = &text[colon + 1..];
    // `host:port` is not a scheme.
    let port_len = rest.bytes().take_while(u8::is_ascii_digit).count();
    if port_len > 0 && rest[port_len..].chars().next().is_none_or(|c| matches!(c, '/' | '?' | '#')) {
        return None;
    }

    let scheme_lower = scheme.to_ascii_lowercase();
    if let Some(page) = InternalPage::from_url(text) {
        return Some(Destination::Internal { page, url: page.url().to_string() });
    }
    match scheme_lower.as_str() {
        "javascript" | "vbscript" => {
            return Some(Destination::Blocked { reason: BlockReason::ScriptUrl });
        }
        "limbo" => {
            return Some(Destination::Internal {
                page: InternalPage::NewTab,
                url: InternalPage::NewTab.url().to_string(),
            });
        }
        "edge" | "chrome" => {
            if opts.developer_mode {
                let url = format!("edge:{rest}");
                return Some(Destination::Navigate { url });
            }
            return Some(Destination::Blocked { reason: BlockReason::EnginePage });
        }
        "about" => {
            let page = rest.trim_start_matches('/').to_ascii_lowercase();
            if page == "blank" || page.starts_with("blank?") || page.starts_with("blank#") {
                return Some(Destination::Navigate { url: "about:blank".into() });
            }
            if opts.developer_mode {
                return Some(Destination::Navigate { url: text.to_string() });
            }
            return Some(Destination::Blocked { reason: BlockReason::EnginePage });
        }
        _ => {}
    }

    let known = NAVIGABLE_SCHEMES.contains(&scheme_lower.as_str());
    // Unknown schemes like `steam://` or `zoommtg://` hand off to the OS, but only
    // when they look deliberate: `scheme://` with no whitespace.
    let deliberate_external = !text.contains(char::is_whitespace) && rest.starts_with("//") && !scheme.contains('.');
    if !known && !deliberate_external {
        return None;
    }
    if text.contains(char::is_whitespace) && matches!(scheme_lower.as_str(), "http" | "https") {
        // "https: is it safe" and friends
        if !rest.starts_with("//") {
            return None;
        }
    }
    let url = Url::parse(text).ok()?;
    if matches!(url.scheme(), "http" | "https") && url.host_str().is_none_or(str::is_empty) {
        return None;
    }
    Some(Destination::Navigate { url: url.into() })
}

/// Returns a navigable URL when the input looks like a host with an optional port/path.
fn host_like_url(text: &str) -> Option<String> {
    let split = text.find(['/', '?', '#']).unwrap_or(text.len());
    let (authority, rest) = text.split_at(split);
    if authority.is_empty() || authority.contains('@') || authority.contains(char::is_whitespace) {
        return None;
    }

    // Split host and port.
    let (host, port) = if let Some(inner) = authority.strip_prefix('[') {
        let end = inner.find(']')?;
        let host = &authority[..end + 2];
        let after = &inner[end + 1..];
        let port = match after.strip_prefix(':') {
            Some(p) => Some(p),
            None if after.is_empty() => None,
            None => return None,
        };
        (host, port)
    } else {
        match authority.rsplit_once(':') {
            Some((h, p)) => (h, Some(p)),
            None => (authority, None),
        }
    };
    if let Some(p) = port {
        if p.is_empty() || p.len() > 5 || !p.bytes().all(|b| b.is_ascii_digit()) {
            return None;
        }
        if p.parse::<u32>().ok()? > 65535 {
            return None;
        }
    }
    if host.is_empty() {
        return None;
    }

    let host_lower = host.to_lowercase();
    let host_trimmed = host_lower.trim_end_matches('.');
    // An explicit port or path makes intranet-style hosts (no public suffix) navigable.
    let has_extra = port.is_some() || rest.starts_with('/');

    let scheme = if host_lower.starts_with('[') {
        // IPv6 literal
        host_lower
            .parse::<std::net::Ipv6Addr>()
            .ok()
            .or_else(|| host_lower.trim_start_matches('[').trim_end_matches(']').parse::<std::net::Ipv6Addr>().ok())?;
        "http"
    } else if is_ipv4(host_trimmed) || host_trimmed == "localhost" || host_trimmed.ends_with(".localhost") {
        // IP literals and localhost are usually dev servers and routers without TLS.
        "http"
    } else if !host_trimmed.contains('.') {
        // Single-label intranet host: only with an explicit port or path.
        if has_extra && valid_labels(host_trimmed) && !host_trimmed.bytes().all(|b| b.is_ascii_digit()) {
            "http"
        } else {
            return None;
        }
    } else {
        if !valid_labels(host_trimmed) {
            return None;
        }
        let ascii = to_ascii_host(host_trimmed)?;
        let tld = ascii.rsplit('.').next().unwrap_or("");
        if tld.is_empty() || tld.bytes().all(|b| b.is_ascii_digit()) {
            return None;
        }
        if has_public_suffix(&ascii) {
            "https"
        } else if has_extra {
            // Unknown TLD (router.lan/, nas.local:5000): treat as an intranet host.
            "http"
        } else {
            return None;
        }
    };

    let candidate = format!("{scheme}://{text}");
    let url = Url::parse(&candidate).ok()?;
    url.host_str()?;
    Some(url.into())
}

fn is_ipv4(host: &str) -> bool {
    let parts: Vec<&str> = host.split('.').collect();
    parts.len() == 4
        && parts.iter().all(|p| {
            !p.is_empty()
                && p.len() <= 3
                && p.bytes().all(|b| b.is_ascii_digit())
                && p.parse::<u16>().is_ok_and(|n| n <= 255)
        })
}

fn valid_labels(host: &str) -> bool {
    host.split('.').all(|label| {
        !label.is_empty()
            && label.chars().count() <= 63
            && !label.starts_with('-')
            && !label.ends_with('-')
            && label.chars().all(|c| c.is_alphanumeric() || c == '-' || c == '_')
    })
}

fn to_ascii_host(host: &str) -> Option<String> {
    if host.is_ascii() {
        return Some(host.to_string());
    }
    let url = Url::parse(&format!("https://{host}/")).ok()?;
    url.host_str().map(str::to_string)
}

/// True when the host ends in a known public suffix and has a registrable label before it.
pub fn has_public_suffix(ascii_host: &str) -> bool {
    let bytes = ascii_host.as_bytes();
    match psl::domain(bytes) {
        Some(domain) => domain.suffix().is_known(),
        None => false,
    }
}

/// eTLD+1 of a host (`mail.google.com` -> `google.com`), used for "same site" checks.
pub fn registrable_domain(host: &str) -> Option<String> {
    let lower = host.to_ascii_lowercase();
    let domain = psl::domain(lower.as_bytes())?;
    if !domain.suffix().is_known() {
        return None;
    }
    std::str::from_utf8(domain.as_bytes()).ok().map(str::to_string)
}

/// If `url` is a Google search results page, returns the query so the focused
/// omnibox can show what the user searched for instead of a long URL.
pub fn search_terms_from_url(url: &str) -> Option<String> {
    let url = Url::parse(url).ok()?;
    let host = url.host_str()?;
    let is_google = host == "www.google.com" || host == "google.com";
    if !is_google || url.path() != "/search" {
        return None;
    }
    url.query_pairs().find(|(k, _)| k == "q").map(|(_, v)| v.into_owned()).filter(|q| !q.is_empty())
}

/// The quiet, unfocused omnibox text: just the site (Safari-style).
pub fn display_host(url: &str) -> String {
    if let Some(page) = InternalPage::from_url(url) {
        return page.title().to_string();
    }
    match Url::parse(url) {
        Ok(u) => match u.scheme() {
            "http" | "https" => {
                let host = u.host_str().unwrap_or_default();
                host.strip_prefix("www.").unwrap_or(host).to_string()
            }
            "file" => u
                .path_segments()
                .and_then(|mut s| s.next_back().map(str::to_string))
                .filter(|s| !s.is_empty())
                .map(|s| percent_encoding::percent_decode_str(&s).decode_utf8_lossy().into_owned())
                .unwrap_or_else(|| "File".into()),
            "about" => url.to_string(),
            "chrome-extension" => "Extension".into(),
            _ => u.scheme().to_string(),
        },
        Err(_) => url.to_string(),
    }
}

/// The origin (`scheme://host[:port]`) used for passwords and permissions.
pub fn origin_of(url: &str) -> Option<String> {
    let u = Url::parse(url).ok()?;
    match u.origin() {
        url::Origin::Tuple(..) => Some(u.origin().ascii_serialization()),
        url::Origin::Opaque(_) => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn nav(input: &str) -> String {
        match classify(input, ClassifyOptions::default()) {
            Some(Destination::Navigate { url }) => url,
            other => panic!("expected navigate for {input:?}, got {other:?}"),
        }
    }

    fn is_search(input: &str) -> bool {
        matches!(classify(input, ClassifyOptions::default()), Some(Destination::Search { .. }))
    }

    #[test]
    fn empty_input_is_none() {
        assert_eq!(classify("", ClassifyOptions::default()), None);
        assert_eq!(classify("   ", ClassifyOptions::default()), None);
        assert_eq!(classify("?", ClassifyOptions::default()), None);
    }

    #[test]
    fn classification_table() {
        let navigate: &[(&str, &str)] = &[
            ("example.com", "https://example.com/"),
            ("Example.COM", "https://example.com/"),
            ("www.google.com", "https://www.google.com/"),
            ("google.com/maps", "https://google.com/maps"),
            ("en.wikipedia.org/wiki/Rust", "https://en.wikipedia.org/wiki/Rust"),
            ("bbc.co.uk", "https://bbc.co.uk/"),
            ("user.github.io", "https://user.github.io/"),
            ("readme.md", "https://readme.md/"),
            ("example.com:8443", "https://example.com:8443/"),
            ("example.com/search?q=a b", "https://example.com/search?q=a%20b"),
            ("example.com.", "https://example.com./"),
            ("sub-domain.example.org", "https://sub-domain.example.org/"),
            ("example.com#frag", "https://example.com/#frag"),
            ("example.com?x=1", "https://example.com/?x=1"),
            ("müller.de", "https://xn--mller-kva.de/"),
            ("localhost", "http://localhost/"),
            ("localhost:3000", "http://localhost:3000/"),
            ("localhost:5173/app", "http://localhost:5173/app"),
            ("app.localhost", "http://app.localhost/"),
            ("127.0.0.1", "http://127.0.0.1/"),
            ("192.168.1.1", "http://192.168.1.1/"),
            ("192.168.1.1:8080/admin", "http://192.168.1.1:8080/admin"),
            ("[::1]", "http://[::1]/"),
            ("[::1]:8080", "http://[::1]:8080/"),
            ("router.lan/", "http://router.lan/"),
            ("nas.local:5000", "http://nas.local:5000/"),
            ("intranet/", "http://intranet/"),
            ("myserver:8080", "http://myserver:8080/"),
            ("http://example.com", "http://example.com/"),
            ("https://example.com/path", "https://example.com/path"),
            ("HTTPS://EXAMPLE.COM", "https://example.com/"),
            ("https://localhost:3000", "https://localhost:3000/"),
            ("file:///C:/Users/me/a.html", "file:///C:/Users/me/a.html"),
            ("C:\\Users\\me\\a.html", "file:///C:/Users/me/a.html"),
            ("C:/Users/me/a.html", "file:///C:/Users/me/a.html"),
            ("\\\\server\\share\\a.pdf", "file://server/share/a.pdf"),
            ("mailto:me@example.com", "mailto:me@example.com"),
            ("tel:+15551234567", "tel:+15551234567"),
            ("about:blank", "about:blank"),
            ("data:text/plain,hi", "data:text/plain,hi"),
            ("steam://run/570", "steam://run/570"),
            (
                "chrome-extension://abcdefghijklmnopabcdefghijklmnop/popup.html",
                "chrome-extension://abcdefghijklmnopabcdefghijklmnop/popup.html",
            ),
            ("view-source:https://example.com", "view-source:https://example.com"),
        ];
        for (input, expected) in navigate {
            assert_eq!(&nav(input), expected, "input {input:?}");
        }

        let searches: &[&str] = &[
            "rust",
            "hello world",
            "what is rust?",
            "example.com?",
            "?example.com",
            "how to use example.com",
            "1.5",
            "3.14159",
            "1.2.3",
            "256.1.1.1",
            "foo.notarealtld",
            "file.txt",
            "C++",
            "c#",
            "note: buy milk",
            "me@example.com",
            "12345",
            "localhost is slow",
            "a.b",
            "https: is it safe",
            "-example.com",
            "example..com",
            "co.uk",
            "com",
            "weather tomorrow",
            "node.js",
        ];
        for input in searches {
            assert!(
                is_search(input),
                "expected search for {input:?}, got {:?}",
                classify(input, ClassifyOptions::default())
            );
        }
    }

    #[test]
    fn js_is_not_a_host_even_though_its_a_tld() {
        // `.js` isn't a TLD, but `.md`, `.io`, `.rs` are; make sure `node.js` searches
        // and `docs.rs` navigates.
        assert!(is_search("node.js"));
        assert_eq!(nav("docs.rs"), "https://docs.rs/");
        assert_eq!(nav("crates.io"), "https://crates.io/");
    }

    #[test]
    fn forced_search_strips_leading_question_mark() {
        match classify("?example.com", ClassifyOptions::default()) {
            Some(Destination::Search { query, url }) => {
                assert_eq!(query, "example.com");
                assert_eq!(url, "https://www.google.com/search?q=example.com");
            }
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn search_url_encoding() {
        assert_eq!(google_search_url("a b&c"), "https://www.google.com/search?q=a+b%26c");
        assert_eq!(google_search_url("ü?"), "https://www.google.com/search?q=%C3%BC%3F");
        assert_eq!(google_search_url("c#"), "https://www.google.com/search?q=c%23");
    }

    #[test]
    fn script_urls_are_blocked() {
        for input in ["javascript:alert(1)", "JavaScript:void(0)", "vbscript:msgbox"] {
            assert_eq!(
                classify(input, ClassifyOptions::default()),
                Some(Destination::Blocked { reason: BlockReason::ScriptUrl }),
                "{input}"
            );
        }
    }

    #[test]
    fn engine_pages_map_to_internal_or_block() {
        let internal = |s: &str| match classify(s, ClassifyOptions::default()) {
            Some(Destination::Internal { page, .. }) => page,
            other => panic!("{s}: {other:?}"),
        };
        assert_eq!(internal("edge://downloads"), InternalPage::Downloads);
        assert_eq!(internal("chrome://downloads/"), InternalPage::Downloads);
        assert_eq!(internal("chrome://settings"), InternalPage::Settings);
        assert_eq!(internal("edge://settings/passwords"), InternalPage::Passwords);
        assert_eq!(internal("chrome://extensions"), InternalPage::Extensions);
        assert_eq!(internal("about:preferences"), InternalPage::Settings);
        assert_eq!(internal("about:addons"), InternalPage::Extensions);
        assert_eq!(internal("about:logins"), InternalPage::Passwords);
        assert_eq!(internal("about:newtab"), InternalPage::NewTab);
        assert_eq!(internal("limbo://settings"), InternalPage::Settings);
        assert_eq!(internal("limbo://history"), InternalPage::History);
        assert_eq!(internal("LIMBO://NEWTAB"), InternalPage::NewTab);

        assert_eq!(
            classify("edge://gpu", ClassifyOptions::default()),
            Some(Destination::Blocked { reason: BlockReason::EnginePage })
        );
        assert_eq!(
            classify("about:config", ClassifyOptions::default()),
            Some(Destination::Blocked { reason: BlockReason::EnginePage })
        );
        assert_eq!(
            classify("edge://process-internals", ClassifyOptions { developer_mode: true }),
            Some(Destination::Navigate { url: "edge://process-internals".into() })
        );
    }

    #[test]
    fn ctrl_enter_wraps_bare_words() {
        assert_eq!(ctrl_enter_url("google").as_deref(), Some("https://www.google.com/"));
        assert_eq!(ctrl_enter_url("github/rust-lang").as_deref(), Some("https://www.github.com/rust-lang"));
        assert_eq!(ctrl_enter_url("example.org").as_deref(), Some("https://www.example.org/"));
        assert_eq!(ctrl_enter_url("www.bing.com").as_deref(), Some("https://www.bing.com/"));
        assert_eq!(ctrl_enter_url("two words"), None);
        assert_eq!(ctrl_enter_url(""), None);
    }

    #[test]
    fn search_terms_are_extracted_from_google_urls() {
        assert_eq!(
            search_terms_from_url("https://www.google.com/search?q=rust+lang&sca_esv=1").as_deref(),
            Some("rust lang")
        );
        assert_eq!(search_terms_from_url("https://www.google.com/maps?q=x"), None);
        assert_eq!(search_terms_from_url("https://example.com/search?q=x"), None);
    }

    #[test]
    fn display_host_is_quiet() {
        assert_eq!(display_host("https://www.google.com/search?q=x"), "google.com");
        assert_eq!(display_host("https://mail.google.com/mail/u/0"), "mail.google.com");
        assert_eq!(display_host("http://localhost:3000/"), "localhost");
        assert_eq!(display_host("limbo://settings"), "Settings");
        assert_eq!(display_host("file:///C:/Users/me/My%20File.pdf"), "My File.pdf");
    }

    #[test]
    fn origins_and_registrable_domains() {
        assert_eq!(origin_of("https://accounts.google.com/signin?x").as_deref(), Some("https://accounts.google.com"));
        assert_eq!(origin_of("http://localhost:3000/a").as_deref(), Some("http://localhost:3000"));
        assert_eq!(origin_of("data:text/plain,hi"), None);
        assert_eq!(registrable_domain("mail.google.com").as_deref(), Some("google.com"));
        assert_eq!(registrable_domain("a.b.bbc.co.uk").as_deref(), Some("bbc.co.uk"));
        assert_eq!(registrable_domain("localhost"), None);
    }
}
