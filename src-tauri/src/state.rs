//! Shared browser state. `Browser` is `Send + Sync` and lives in an `Arc`.
//!
//! Locking rule: never hold `inner` while calling into Tauri or WebView2
//! (`with_webview` runs synchronously when already on the main thread, and
//! WebView2 callbacks lock `inner` too). Copy what you need, drop the guard, act.

use std::sync::{Arc, OnceLock};
use std::time::Instant;

use limbo_core::lifecycle::{Lifecycle, TabFlags, TabState};
use limbo_core::memory::SamplingMode;
use limbo_core::omnibox::{self, InternalPage};
use limbo_core::sessions::SessionTab;
use limbo_core::settings::Settings;
use parking_lot::{Condvar, Mutex};
use serde::Serialize;
use tauri::{AppHandle, Emitter, Webview, Window, Wry};

use crate::db_worker::DbHandle;
use crate::paths::Paths;

pub type TabId = u64;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Security {
    Secure,
    Insecure,
    Internal,
    File,
}

impl Security {
    pub fn of(url: &str) -> Security {
        let lower = url.to_ascii_lowercase();
        if lower.starts_with("https://") {
            Security::Secure
        } else if lower.starts_with("file:") {
            Security::File
        } else if lower.starts_with("http://") {
            Security::Insecure
        } else {
            Security::Internal
        }
    }
}

/// Everything the UI knows about a tab.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TabInfo {
    pub id: TabId,
    pub url: String,
    pub title: String,
    /// `data:` URL of the page icon.
    pub favicon: Option<String>,
    pub loading: bool,
    /// 0..1, a coarse signal the UI animates between.
    pub progress: f32,
    pub can_go_back: bool,
    pub can_go_forward: bool,
    pub audible: bool,
    pub muted: bool,
    pub pinned: bool,
    pub private: bool,
    pub state: TabState,
    pub security: Security,
    pub internal: Option<InternalPage>,
    pub zoom: f64,
    pub crashed: bool,
    pub has_snapshot: bool,
    /// Unfocused omnibox text.
    pub display_host: String,
    /// Focused omnibox text on Google results pages.
    pub search_terms: Option<String>,
    /// An extension store detail page: show "Add to Limbo".
    pub store_extension: Option<StoreExtension>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct StoreExtension {
    pub store: limbo_core::extensions::store::Store,
    pub id: String,
    pub installed: bool,
}

impl TabInfo {
    pub fn new(id: TabId, url: &str, private: bool) -> TabInfo {
        let mut t = TabInfo {
            id,
            url: String::new(),
            title: String::new(),
            favicon: None,
            loading: false,
            progress: 0.0,
            can_go_back: false,
            can_go_forward: false,
            audible: false,
            muted: false,
            pinned: false,
            private,
            state: TabState::Hidden,
            security: Security::Internal,
            internal: None,
            zoom: 1.0,
            crashed: false,
            has_snapshot: false,
            display_host: String::new(),
            search_terms: None,
            store_extension: None,
        };
        t.set_url(url);
        t
    }

    /// Updates the URL and everything derived from it.
    pub fn set_url(&mut self, url: &str) {
        self.url = url.to_string();
        self.internal = InternalPage::from_url(url);
        self.security = Security::of(url);
        self.display_host = omnibox::display_host(url);
        self.search_terms = omnibox::search_terms_from_url(url);
        if let Some(page) = self.internal {
            self.title = page.title().to_string();
        }
        self.store_extension = limbo_core::extensions::store::parse_detail_url(url).map(|(store, id)| StoreExtension {
            store,
            id,
            installed: false,
        });
    }
}

pub struct Tab {
    pub info: TabInfo,
    pub webview: Option<Webview<Wry>>,
    pub flags: TabFlags,
    /// The next navigation came from the omnibox (history transition "typed").
    pub typed_navigation: bool,
    /// The user typed a file:// URL, so file navigation is allowed in this tab.
    pub allow_file: bool,
    /// WebView2 main frame id, used to attribute renderer memory to this tab.
    pub main_frame_id: Option<u32>,
    /// How the navigation in flight started (for the history record).
    pub pending_transition: Option<limbo_core::frecency::Transition>,
}

impl Tab {
    pub fn new(info: TabInfo) -> Tab {
        Tab {
            info,
            webview: None,
            flags: TabFlags::default(),
            typed_navigation: false,
            allow_file: false,
            main_frame_id: None,
            pending_transition: None,
        }
    }
}

/// Space the UI reserves around the page (top bar, sidebar, find bar), in CSS px.
#[derive(Debug, Clone, Copy, Default, PartialEq, serde::Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Insets {
    pub top: f64,
    pub left: f64,
    pub right: f64,
    pub bottom: f64,
}

pub struct Inner {
    pub tabs: Vec<Tab>,
    pub active: Option<TabId>,
    pub next_id: TabId,
    pub lifecycle: Lifecycle,
    pub settings: Settings,
    pub insets: Insets,
    /// Recently closed tabs (Ctrl+Shift+T), newest last, at most 25.
    pub closed: Vec<(usize, SessionTab)>,
    /// A snapshot overlay is covering the page area.
    pub overlay: bool,
    pub fullscreen: bool,
    pub sampling: SamplingMode,
    pub minimized: bool,
    pub ui_ready: bool,
}

impl Inner {
    pub fn tab(&self, id: TabId) -> Option<&Tab> {
        self.tabs.iter().find(|t| t.info.id == id)
    }

    pub fn tab_mut(&mut self, id: TabId) -> Option<&mut Tab> {
        self.tabs.iter_mut().find(|t| t.info.id == id)
    }

    pub fn index_of(&self, id: TabId) -> Option<usize> {
        self.tabs.iter().position(|t| t.info.id == id)
    }

    pub fn webview(&self, id: TabId) -> Option<Webview<Wry>> {
        self.tab(id).and_then(|t| t.webview.clone())
    }

    pub fn active_webview(&self) -> Option<Webview<Wry>> {
        self.active.and_then(|id| self.webview(id))
    }

    pub fn alloc_id(&mut self) -> TabId {
        self.next_id += 1;
        self.next_id
    }
}

/// Wakes the lifecycle driver when something changes.
#[derive(Default)]
pub struct Signal {
    pending: Mutex<bool>,
    cv: Condvar,
}

impl Signal {
    pub fn notify(&self) {
        *self.pending.lock() = true;
        self.cv.notify_all();
    }

    /// Waits until notified or `timeout` passes. Returns true if notified.
    pub fn wait(&self, timeout: Option<std::time::Duration>) -> bool {
        let mut pending = self.pending.lock();
        if !*pending {
            match timeout {
                Some(t) => {
                    self.cv.wait_for(&mut pending, t);
                }
                None => self.cv.wait(&mut pending),
            }
        }
        std::mem::replace(&mut *pending, false)
    }
}

pub struct Browser {
    pub app: AppHandle<Wry>,
    pub window: Window<Wry>,
    pub ui: OnceLock<Webview<Wry>>,
    pub paths: Paths,
    pub db: DbHandle,
    pub inner: Mutex<Inner>,
    /// Wakes the lifecycle thread.
    pub lifecycle_signal: Signal,
    /// Wakes the memory sampler.
    pub sampler_signal: Signal,
    started: Instant,
    pub env: OnceLock<crate::webview2::SendEnv>,
    pub http: ureq::Agent,
    pub protector: Arc<crate::passwords::Protector>,
    /// Pending callbacks keyed by id (permission prompts, context menus, ...).
    pub pending: crate::pending::Pending,
}

pub type SharedBrowser = Arc<Browser>;

impl Browser {
    pub fn new(app: AppHandle<Wry>, window: Window<Wry>, paths: Paths, db: DbHandle, settings: Settings) -> Browser {
        let lifecycle = Lifecycle::new(settings.memory);
        Browser {
            app,
            window,
            ui: OnceLock::new(),
            paths,
            db,
            inner: Mutex::new(Inner {
                tabs: Vec::new(),
                active: None,
                next_id: 0,
                lifecycle,
                settings,
                insets: Insets { top: 44.0, ..Default::default() },
                closed: Vec::new(),
                overlay: false,
                fullscreen: false,
                sampling: SamplingMode::Pill,
                minimized: false,
                ui_ready: false,
            }),
            lifecycle_signal: Signal::default(),
            sampler_signal: Signal::default(),
            started: Instant::now(),
            env: OnceLock::new(),
            http: crate::http::agent(),
            protector: Arc::new(crate::passwords::Protector::new()),
            pending: crate::pending::Pending::default(),
        }
    }

    /// Monotonic milliseconds for the lifecycle state machine.
    pub fn now_ms(&self) -> u64 {
        self.started.elapsed().as_millis() as u64
    }

    /// Sends an event to the UI webview only.
    pub fn emit<S: Serialize + Clone>(&self, event: &str, payload: S) {
        if let Err(e) = self.app.emit_to("ui", event, payload) {
            log::warn!("emit {event}: {e}");
        }
    }

    pub fn toast(&self, message: impl Into<String>) {
        self.emit("toast", serde_json::json!({ "message": message.into() }));
    }

    /// Emits the current info of a tab.
    pub fn emit_tab(&self, id: TabId) {
        let info = {
            let inner = self.inner.lock();
            inner.tab(id).map(|t| {
                let mut i = t.info.clone();
                i.state = inner.lifecycle.state(id).unwrap_or(i.state);
                i
            })
        };
        if let Some(info) = info {
            self.emit("tab:updated", info);
        }
    }

    pub fn settings(&self) -> Settings {
        self.inner.lock().settings.clone()
    }

    pub fn tab_infos(&self) -> Vec<TabInfo> {
        let inner = self.inner.lock();
        inner
            .tabs
            .iter()
            .map(|t| {
                let mut i = t.info.clone();
                i.state = inner.lifecycle.state(t.info.id).unwrap_or(i.state);
                i
            })
            .collect()
    }
}
