//! Omnibox suggestions: local results are returned at once; Google's arrive
//! later as a `suggest:results` event (and are dropped if the text changed).

use std::collections::VecDeque;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use limbo_core::omnibox::ClassifyOptions;
use limbo_core::suggest::{self, LocalMatch, MergeInput, OpenTabMatch, Suggestion};
use parking_lot::Mutex;
use serde::Serialize;

use crate::state::SharedBrowser;

static GENERATION: AtomicU64 = AtomicU64::new(0);
/// Recent Google responses, so backspacing is instant and cheap.
static CACHE: Mutex<VecDeque<(String, Vec<String>)>> = Mutex::new(VecDeque::new());
const CACHE_SIZE: usize = 32;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Results {
    pub text: String,
    pub rows: Vec<Suggestion>,
}

struct Local {
    hosts: Vec<String>,
    matches: Vec<LocalMatch>,
}

fn open_tabs(b: &SharedBrowser) -> Vec<OpenTabMatch> {
    let inner = b.inner.lock();
    inner
        .tabs
        .iter()
        .filter(|t| Some(t.info.id) != inner.active && t.info.internal.is_none())
        .map(|t| OpenTabMatch { tab_id: t.info.id, url: t.info.url.clone(), title: t.info.title.clone() })
        .collect()
}

fn merge(b: &SharedBrowser, text: &str, local: &Local, remote: &[String], deleting: bool) -> Vec<Suggestion> {
    let developer_mode = b.settings().developer_mode;
    let tabs = open_tabs(b);
    suggest::merge(&MergeInput {
        text,
        options: ClassifyOptions { developer_mode },
        autofill_hosts: &local.hosts,
        local: &local.matches,
        open_tabs: &tabs,
        remote,
        deleting,
    })
}

fn cached(text: &str) -> Option<Vec<String>> {
    CACHE.lock().iter().find(|(t, _)| t == text).map(|(_, r)| r.clone())
}

pub async fn query(b: &SharedBrowser, text: String, deleting: bool) -> Result<Vec<Suggestion>, String> {
    let generation = GENERATION.fetch_add(1, Ordering::AcqRel) + 1;
    let t = text.clone();
    let local =
        b.db.call(move |db| Local {
            hosts: limbo_core::history::autofill_hosts(db.conn(), &t, 3).unwrap_or_default(),
            matches: limbo_core::history::search(db.conn(), &t, 8).unwrap_or_default(),
        })
        .await?;
    let remote_cached = cached(&text);
    let rows = merge(b, &text, &local, remote_cached.as_deref().unwrap_or(&[]), deleting);

    let (enabled, private) = {
        let inner = b.inner.lock();
        let private = inner.active.and_then(|a| inner.tab(a)).map(|t| t.info.private).unwrap_or(false);
        (inner.settings.show_google_suggestions, private)
    };
    let wants_remote = enabled && !private && remote_cached.is_none() && !text.trim().is_empty() && text.len() < 200;
    if wants_remote {
        let b2 = b.clone();
        std::thread::spawn(move || {
            // Let a fast typist finish before hitting the network.
            std::thread::sleep(Duration::from_millis(60));
            if GENERATION.load(Ordering::Acquire) != generation {
                return;
            }
            let url = suggest::google_suggest_url(text.trim());
            let remote = match crate::http::get_string(&b2.http, &url, Duration::from_millis(1500)) {
                Ok(body) => suggest::parse_google_suggestions(&body),
                Err(e) => {
                    log::debug!("suggestions unavailable: {e}");
                    return;
                }
            };
            {
                let mut cache = CACHE.lock();
                cache.push_back((text.clone(), remote.clone()));
                while cache.len() > CACHE_SIZE {
                    cache.pop_front();
                }
            }
            if GENERATION.load(Ordering::Acquire) != generation {
                return;
            }
            let rows = merge(&b2, &text, &local, &remote, deleting);
            b2.emit("suggest:results", Results { text, rows });
        });
    }
    Ok(rows)
}

/// Shift+Delete on a history suggestion.
pub async fn remove(b: &SharedBrowser, url: String) -> Result<(), String> {
    b.db.run(move |db| limbo_core::history::delete_url(db.conn_mut(), &url, limbo_core::db::now_us())).await
}
