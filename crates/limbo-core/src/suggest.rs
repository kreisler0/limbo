//! Omnibox suggestions: merges local history/bookmark matches (instant) with
//! Google's suggestions (network, may never arrive) into at most 8 rows.

use serde::Serialize;

use crate::omnibox::{self, ClassifyOptions, Destination};

pub const MAX_ROWS: usize = 8;
/// Local rows shown before Google's suggestions.
const LOCAL_FIRST: usize = 3;

/// Google's suggest endpoint (unofficial; failures are silent).
pub fn google_suggest_url(query: &str) -> String {
    let q: String = url::form_urlencoded::byte_serialize(query.as_bytes()).collect();
    format!("https://suggestqueries.google.com/complete/search?client=firefox&q={q}")
}

/// Parses `["query", ["s1", "s2", ...], ...]`. Anything else yields no suggestions.
pub fn parse_google_suggestions(body: &str) -> Vec<String> {
    let Ok(serde_json::Value::Array(parts)) = serde_json::from_str::<serde_json::Value>(body) else {
        return Vec::new();
    };
    match parts.get(1) {
        Some(serde_json::Value::Array(items)) => items
            .iter()
            .filter_map(|v| v.as_str())
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_string)
            .collect(),
        _ => Vec::new(),
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum SuggestionKind {
    /// Go to the URL the input classifies as (or the inline-autocompleted URL).
    Navigate,
    /// Search Google.
    Search,
    History,
    Bookmark,
    /// Switch to an already open tab.
    OpenTab,
    Internal,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Suggestion {
    pub kind: SuggestionKind,
    /// Main text (page title or search query).
    pub title: String,
    /// Where Enter goes.
    pub url: String,
    /// For inline autocomplete: the text to append after what the user typed.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub completion: Option<String>,
    /// Tab id for `OpenTab`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tab_id: Option<u64>,
}

/// A history/bookmark match from the local database, best first.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LocalMatch {
    pub url: String,
    pub title: String,
    pub frecency: i64,
    pub bookmarked: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OpenTabMatch {
    pub tab_id: u64,
    pub url: String,
    pub title: String,
}

/// Strips `https://`, `http://` and `www.` for prefix comparisons.
pub fn strip_for_autofill(s: &str) -> &str {
    let s = s.strip_prefix("https://").or_else(|| s.strip_prefix("http://")).unwrap_or(s);
    s.strip_prefix("www.").unwrap_or(s)
}

/// Inline autocomplete: if the input is a prefix of a known host (best first), returns
/// `(url, completion)`, e.g. `"goo"` -> `("https://google.com/", "gle.com/")`.
///
/// `hosts` are origin-level candidates like `"https://www.google.com"`.
pub fn inline_autocomplete(input: &str, hosts: &[String]) -> Option<(String, String)> {
    if input.is_empty() || input.contains(char::is_whitespace) || input.ends_with('?') {
        return None;
    }
    let typed_lower = input.to_lowercase();
    let typed_core = strip_for_autofill(&typed_lower);
    if typed_core.is_empty() {
        return None;
    }
    for host in hosts {
        let host_lower = host.to_lowercase();
        let display = format!("{}/", strip_for_autofill(host_lower.trim_end_matches('/')));
        if display.len() > typed_core.len() && display.starts_with(typed_core) {
            let completion = display[typed_core.len()..].to_string();
            let url = if host_lower.contains("://") {
                format!("{}/", host_lower.trim_end_matches('/'))
            } else {
                format!("https://{}", display)
            };
            return Some((url, completion));
        }
    }
    None
}

/// Inputs to [`merge`].
#[derive(Debug, Clone, Default)]
pub struct MergeInput<'a> {
    pub text: &'a str,
    pub options: ClassifyOptions,
    /// Origin-level candidates for inline autocomplete, best first.
    pub autofill_hosts: &'a [String],
    pub local: &'a [LocalMatch],
    pub open_tabs: &'a [OpenTabMatch],
    pub remote: &'a [String],
    /// The user just deleted text: don't inline-autocomplete (it would re-add it).
    pub deleting: bool,
}

/// Builds the suggestion list: the top row (autocomplete / go to / search), up to 3
/// local rows, then Google suggestions, then any remaining local rows. Max 8.
pub fn merge(input: &MergeInput<'_>) -> Vec<Suggestion> {
    let text = input.text.trim();
    let mut rows: Vec<Suggestion> = Vec::with_capacity(MAX_ROWS);
    let Some(dest) = omnibox::classify(text, input.options) else {
        return rows;
    };

    // Top row.
    let autofill = if input.deleting { None } else { inline_autocomplete(text, input.autofill_hosts) };
    let top = match (&autofill, &dest) {
        (Some((url, completion)), Destination::Search { .. } | Destination::Navigate { .. }) => Suggestion {
            kind: SuggestionKind::Navigate,
            title: format!("{text}{completion}"),
            url: url.clone(),
            completion: Some(completion.clone()),
            tab_id: None,
        },
        (_, Destination::Navigate { url }) => Suggestion {
            kind: SuggestionKind::Navigate,
            title: url.clone(),
            url: url.clone(),
            completion: None,
            tab_id: None,
        },
        (_, Destination::Search { query, url }) => Suggestion {
            kind: SuggestionKind::Search,
            title: query.clone(),
            url: url.clone(),
            completion: None,
            tab_id: None,
        },
        (_, Destination::Internal { page, url }) => Suggestion {
            kind: SuggestionKind::Internal,
            title: page.title().to_string(),
            url: url.clone(),
            completion: None,
            tab_id: None,
        },
        (_, Destination::Blocked { .. }) => return rows,
    };
    let top_is_search = top.kind == SuggestionKind::Search;
    rows.push(top);

    let needle = text.to_lowercase();
    let mut seen_urls: Vec<String> = rows.iter().map(|r| normalize_url(&r.url)).collect();
    // A search row equal to the input is only noise when the top row already searches;
    // after an autocompleted URL it's the way to search instead.
    let mut seen_queries: Vec<String> = if top_is_search { vec![needle.clone()] } else { Vec::new() };

    // Open tabs that match go right after the top row.
    for tab in input.open_tabs {
        if rows.len() >= 2 {
            break;
        }
        let hay = format!("{} {}", tab.title.to_lowercase(), tab.url.to_lowercase());
        if !needle.is_empty() && hay.contains(&needle) && !seen_urls.contains(&normalize_url(&tab.url)) {
            seen_urls.push(normalize_url(&tab.url));
            rows.push(Suggestion {
                kind: SuggestionKind::OpenTab,
                title: tab.title.clone(),
                url: tab.url.clone(),
                completion: None,
                tab_id: Some(tab.tab_id),
            });
        }
    }

    let push_local = |rows: &mut Vec<Suggestion>, seen: &mut Vec<String>, m: &LocalMatch| {
        seen.push(normalize_url(&m.url));
        rows.push(Suggestion {
            kind: if m.bookmarked { SuggestionKind::Bookmark } else { SuggestionKind::History },
            title: if m.title.is_empty() { m.url.clone() } else { m.title.clone() },
            url: m.url.clone(),
            completion: None,
            tab_id: None,
        });
    };

    let mut local = input.local.iter();
    let mut taken = 0;
    while taken < LOCAL_FIRST && rows.len() < MAX_ROWS {
        let Some(m) = local.next() else { break };
        if seen_urls.contains(&normalize_url(&m.url)) {
            continue;
        }
        push_local(&mut rows, &mut seen_urls, m);
        taken += 1;
    }

    for s in input.remote {
        if rows.len() >= MAX_ROWS {
            break;
        }
        let key = s.to_lowercase();
        if seen_queries.contains(&key) {
            continue;
        }
        seen_queries.push(key);
        rows.push(Suggestion {
            kind: SuggestionKind::Search,
            title: s.clone(),
            url: omnibox::google_search_url(s),
            completion: None,
            tab_id: None,
        });
    }

    for m in local {
        if rows.len() >= MAX_ROWS {
            break;
        }
        if seen_urls.contains(&normalize_url(&m.url)) {
            continue;
        }
        push_local(&mut rows, &mut seen_urls, m);
    }
    rows
}

fn normalize_url(url: &str) -> String {
    let lower = url.to_lowercase();
    strip_for_autofill(lower.trim_end_matches('/')).to_string()
}

/// Builds an FTS5 query for local matching: each whitespace-separated token must
/// match as a prefix. Tokens are quoted so FTS syntax in user input is inert.
pub fn fts_query(text: &str) -> Option<String> {
    let tokens: Vec<String> = text
        .split(|c: char| c.is_whitespace() || matches!(c, '/' | ':' | '.' | '?' | '&' | '=' | '#' | '-' | '_'))
        .filter(|t| !t.is_empty())
        .map(|t| format!("\"{}\"*", t.replace('"', "\"\"")))
        .collect();
    if tokens.is_empty() { None } else { Some(tokens.join(" ")) }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lm(url: &str, title: &str, bookmarked: bool) -> LocalMatch {
        LocalMatch { url: url.into(), title: title.into(), frecency: 100, bookmarked }
    }

    #[test]
    fn parses_google_response() {
        let body = r#"["rust",["rust","rust game","rust lang",""],[],{"google:suggesttype":[]}]"#;
        assert_eq!(parse_google_suggestions(body), vec!["rust", "rust game", "rust lang"]);
        assert!(parse_google_suggestions("not json").is_empty());
        assert!(parse_google_suggestions("{}").is_empty());
        assert!(parse_google_suggestions(r#"["x"]"#).is_empty());
    }

    #[test]
    fn suggest_url() {
        assert_eq!(
            google_suggest_url("a b"),
            "https://suggestqueries.google.com/complete/search?client=firefox&q=a+b"
        );
    }

    #[test]
    fn autocomplete_hosts() {
        let hosts = vec!["https://www.google.com".to_string(), "https://github.com".to_string()];
        assert_eq!(
            inline_autocomplete("goo", &hosts),
            Some(("https://www.google.com/".into(), "gle.com/".into()))
        );
        assert_eq!(inline_autocomplete("www.goo", &hosts).map(|x| x.1), Some("gle.com/".into()));
        assert_eq!(inline_autocomplete("git", &hosts).map(|x| x.0), Some("https://github.com/".into()));
        assert_eq!(inline_autocomplete("github.com/", &hosts), None, "nothing left to complete");
        assert_eq!(inline_autocomplete("go ogle", &hosts), None);
        assert_eq!(inline_autocomplete("x", &hosts), None);
        assert_eq!(inline_autocomplete("", &hosts), None);
    }

    #[test]
    fn merge_orders_rows() {
        let hosts = vec!["https://www.rust-lang.org".to_string()];
        let local = vec![
            lm("https://www.rust-lang.org/", "Rust", true),
            lm("https://doc.rust-lang.org/book/", "The Book", false),
            lm("https://docs.rs/", "Docs.rs", false),
            lm("https://crates.io/", "crates.io", false),
            lm("https://blog.rust-lang.org/", "Rust Blog", false),
        ];
        let remote = vec!["rust".to_string(), "rust game".to_string(), "rust lang".to_string()];
        let rows = merge(&MergeInput {
            text: "rust",
            autofill_hosts: &hosts,
            local: &local,
            remote: &remote,
            ..Default::default()
        });
        assert_eq!(rows[0].kind, SuggestionKind::Navigate);
        assert_eq!(rows[0].completion.as_deref(), Some("-lang.org/"));
        // The autocompleted URL isn't repeated as a history row.
        assert!(!rows[1..].iter().any(|r| r.url == "https://www.rust-lang.org/"));
        assert_eq!(rows[1].title, "The Book");
        assert_eq!(rows[2].title, "Docs.rs");
        assert_eq!(rows[3].title, "crates.io");
        // Google suggestions next; "rust" (== input) is kept because the top row navigates.
        assert_eq!(rows[4].kind, SuggestionKind::Search);
        assert_eq!(rows.len(), MAX_ROWS);
    }

    #[test]
    fn merge_search_top_dedupes_remote_equal_to_input() {
        let remote = vec!["weather".to_string(), "weather tomorrow".to_string()];
        let rows = merge(&MergeInput { text: "weather", remote: &remote, ..Default::default() });
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].kind, SuggestionKind::Search);
        assert_eq!(rows[0].url, "https://www.google.com/search?q=weather");
        assert_eq!(rows[1].title, "weather tomorrow");
    }

    #[test]
    fn merge_deleting_disables_autofill() {
        let hosts = vec!["https://github.com".to_string()];
        let rows = merge(&MergeInput { text: "git", autofill_hosts: &hosts, deleting: true, ..Default::default() });
        assert_eq!(rows[0].kind, SuggestionKind::Search);
    }

    #[test]
    fn merge_open_tabs_and_blocked() {
        let tabs = vec![OpenTabMatch { tab_id: 7, url: "https://mail.google.com/".into(), title: "Inbox".into() }];
        let rows = merge(&MergeInput { text: "inbox", open_tabs: &tabs, ..Default::default() });
        assert_eq!(rows[1].kind, SuggestionKind::OpenTab);
        assert_eq!(rows[1].tab_id, Some(7));
        assert!(merge(&MergeInput { text: "javascript:alert(1)", ..Default::default() }).is_empty());
        assert!(merge(&MergeInput { text: "  ", ..Default::default() }).is_empty());
    }

    #[test]
    fn fts_queries_are_quoted_prefixes() {
        assert_eq!(fts_query("rust book").as_deref(), Some("\"rust\"* \"book\"*"));
        assert_eq!(fts_query("github.com/rust").as_deref(), Some("\"github\"* \"com\"* \"rust\"*"));
        assert_eq!(fts_query("a\"b").as_deref(), Some("\"a\"\"b\"*"));
        assert_eq!(fts_query(" / "), None);
        assert_eq!(fts_query("NEAR(x) OR y").as_deref(), Some("\"NEAR(x)\"* \"OR\"* \"y\"*"));
    }
}
