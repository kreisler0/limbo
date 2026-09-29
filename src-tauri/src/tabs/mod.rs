//! Tab management: one child webview per open web tab, all sharing one
//! WebView2 environment. Internal pages (New Tab, Settings, History, ...) have
//! no webview at all: the UI draws them, which is why a new tab costs no RAM.

pub mod events;
pub mod lifecycle;

use std::sync::Arc;

use limbo_core::lifecycle::{Action, TabFlags};
use limbo_core::omnibox::{self, ClassifyOptions, Destination, InternalPage};
use limbo_core::sessions::SessionTab;
use serde::Serialize;
use tauri::webview::{NewWindowResponse, WebviewBuilder};
use tauri::{LogicalPosition, LogicalSize, PhysicalPosition, PhysicalSize, Rect, Webview, WebviewUrl, Wry};

use crate::state::{Browser, SharedBrowser, Tab, TabId, TabInfo};
use crate::webview2;

pub const NEW_TAB_URL: &str = "limbo://newtab";
const MAX_CLOSED: usize = 25;

#[derive(Debug, Clone, Default, serde::Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct CreateOptions {
    /// Open without switching to it.
    pub background: bool,
    /// Position in the strip (default: after the active tab).
    pub index: Option<usize>,
    pub private: bool,
    /// Insert next to this tab (links opened from a page).
    pub opener: Option<TabId>,
    /// The URL came from the omnibox (history "typed" transition).
    pub typed: bool,
    pub pinned: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TabCreated {
    pub tab: TabInfo,
    pub index: usize,
}

/// The page area in physical pixels, rounded to device pixels.
pub fn content_rect(b: &Browser) -> Option<Rect> {
    let size = b.window.inner_size().ok()?;
    let scale = b.window.scale_factor().ok()?;
    let (insets, fullscreen) = {
        let inner = b.inner.lock();
        (inner.insets, inner.fullscreen)
    };
    let insets = if fullscreen { Default::default() } else { insets };
    let x = (insets.left * scale).round() as i32;
    let y = (insets.top * scale).round() as i32;
    let w = (size.width as i32 - x - (insets.right * scale).round() as i32).max(1);
    let h = (size.height as i32 - y - (insets.bottom * scale).round() as i32).max(1);
    Some(Rect { position: PhysicalPosition::new(x, y).into(), size: PhysicalSize::new(w as u32, h as u32).into() })
}

/// Moves the active tab's webview onto the page area (after resizes and inset changes).
pub fn place_active(b: &Browser) {
    let Some(rect) = content_rect(b) else { return };
    let wv = b.inner.lock().active_webview();
    if let Some(wv) = wv {
        let _ = wv.set_bounds(rect);
    }
}

fn theme_background(b: &Browser) -> (u8, u8, u8) {
    use limbo_core::settings::Theme;
    let dark = match b.settings().theme {
        Theme::Dark => true,
        Theme::Light => false,
        Theme::System => matches!(b.window.theme(), Ok(tauri::Theme::Dark)),
    };
    // --bg in tokens.css: no white flash on dark pages and vice versa.
    if dark { (0x19, 0x19, 0x19) } else { (0xFF, 0xFF, 0xFF) }
}

/// Creates the webview for a tab. Call from a worker/async thread, never from a
/// WebView2 callback (WebView2 re-entrancy would deadlock).
pub fn create_webview(b: &SharedBrowser, id: TabId, url: &str, visible: bool) -> Result<Webview<Wry>, String> {
    let (settings, private) = {
        let inner = b.inner.lock();
        let private = inner.tab(id).map(|t| t.info.private).unwrap_or(false);
        (inner.settings.clone(), private)
    };
    let parsed: url::Url = url.parse().map_err(|e| format!("bad url {url}: {e}"))?;
    let label = format!("tab-{id}");
    let bg = theme_background(b);
    let nav_b = Arc::downgrade(b);
    let win_b = Arc::downgrade(b);
    let mut builder = WebviewBuilder::new(&label, WebviewUrl::External(parsed))
        .data_directory(b.paths.profile.clone())
        .additional_browser_args(&settings.browser_args())
        .browser_extensions_enabled(true)
        .incognito(private)
        .background_color(tauri::window::Color(bg.0, bg.1, bg.2, 255))
        .zoom_hotkeys_enabled(false)
        .general_autofill_enabled(false)
        .focused(visible)
        .initialization_script(crate::passwords::AUTOFILL_JS)
        .on_navigation(move |url| match nav_b.upgrade() {
            Some(b) => navigation_allowed(&b, id, url),
            None => false,
        })
        .on_new_window(move |url, features| match win_b.upgrade() {
            Some(b) => on_new_window(&b, id, url, features),
            None => NewWindowResponse::Deny,
        });
    #[cfg(feature = "devtools")]
    {
        builder = builder.devtools(settings.developer_mode);
    }
    if let Some(env) = b.env.get() {
        builder = builder.with_environment(env.0.clone());
    }
    // Background tabs are created at zero size so nothing flashes, then hidden.
    let (pos, size) = match (visible, content_rect(b)) {
        (true, Some(r)) => (r.position, r.size),
        (_, r) => (
            r.map(|r| r.position).unwrap_or_else(|| LogicalPosition::new(0.0, 44.0).into()),
            LogicalSize::new(0.0, 0.0).into(),
        ),
    };
    let wv = b.window.add_child(builder, pos, size).map_err(|e| e.to_string())?;
    if !visible {
        let _ = wv.hide();
    }
    events::wire(b, id, &wv, &settings);
    Ok(wv)
}

/// Blocks tab webviews from reaching Limbo's own UI origin, and `file:` unless
/// the user typed a file URL into this tab.
fn navigation_allowed(b: &Browser, id: TabId, url: &url::Url) -> bool {
    let host = url.host_str().unwrap_or("");
    if matches!(url.scheme(), "tauri" | "ipc") || host == "tauri.localhost" || host == "ipc.localhost" {
        log::warn!("blocked tab {id} navigating to the UI origin");
        return false;
    }
    if url.scheme() == "file" {
        return b.inner.lock().tab(id).map(|t| t.allow_file).unwrap_or(false);
    }
    true
}

/// `window.open` / `target=_blank` open a tab next to the opener. Real popups
/// (explicit size, e.g. "Sign in with Google") get a small window that keeps
/// `window.opener`, which OAuth flows need.
fn on_new_window(
    b: &SharedBrowser,
    opener: TabId,
    url: url::Url,
    features: tauri::webview::NewWindowFeatures,
) -> NewWindowResponse<Wry> {
    if features.size().is_some() {
        match crate::popup::open(b, opener, &url, features) {
            Ok(window) => return NewWindowResponse::Create { window },
            Err(e) => log::warn!("popup window failed, opening a tab instead: {e}"),
        }
    }
    let b2 = b.clone();
    let private = b.inner.lock().tab(opener).map(|t| t.info.private).unwrap_or(false);
    tauri::async_runtime::spawn(async move {
        let opts = CreateOptions { opener: Some(opener), private, ..Default::default() };
        if let Err(e) = create(&b2, Some(url.to_string()), opts).await {
            log::warn!("new tab from window.open failed: {e}");
        }
    });
    NewWindowResponse::Deny
}

/// Opens a tab. Web URLs get a webview; internal pages don't.
pub async fn create(b: &SharedBrowser, url: Option<String>, opts: CreateOptions) -> Result<TabId, String> {
    let url = url.unwrap_or_else(|| NEW_TAB_URL.to_string());
    let internal = InternalPage::from_url(&url).is_some();
    let (id, index, info) = {
        let mut inner = b.inner.lock();
        let id = inner.alloc_id();
        let mut info = TabInfo::new(id, &url, opts.private);
        info.pinned = opts.pinned;
        if !internal {
            info.loading = true;
            info.progress = 0.05;
        }
        let anchor = opts.opener.or(inner.active).and_then(|a| inner.index_of(a));
        let pinned_count = inner.tabs.iter().filter(|t| t.info.pinned).count();
        let index = opts
            .index
            .or_else(|| anchor.map(|i| i + 1))
            .unwrap_or(inner.tabs.len())
            .min(inner.tabs.len())
            .max(if opts.pinned { 0 } else { pinned_count });
        let mut tab = Tab::new(info.clone());
        tab.typed_navigation = opts.typed;
        tab.allow_file = url.starts_with("file:");
        tab.flags = TabFlags { pinned: opts.pinned, ..Default::default() };
        inner.tabs.insert(index, tab);
        let now = b.now_ms();
        // Internal pages have no webview: to the lifecycle they are "discarded".
        let actions = inner.lifecycle.add(id, TabFlags { pinned: opts.pinned, ..Default::default() }, internal, now);
        drop(inner);
        apply_actions(b, actions);
        (id, index, info)
    };
    b.emit("tab:created", TabCreated { tab: info, index });

    if !internal {
        let wv = create_webview(b, id, &url, !opts.background)?;
        let mut inner = b.inner.lock();
        match inner.tab_mut(id) {
            Some(t) => t.webview = Some(wv),
            None => {
                // Closed while the webview was being created.
                drop(inner);
                let _ = wv.close();
                return Err("tab closed".into());
            }
        }
    }
    if !opts.background {
        activate(b, id, !internal).await;
    }
    crate::session::schedule_save(b);
    Ok(id)
}

/// Switches to a tab. `focus_page` moves keyboard focus into the page.
pub async fn activate(b: &SharedBrowser, id: TabId, focus_page: bool) {
    let actions = {
        let mut inner = b.inner.lock();
        if inner.tab(id).is_none() {
            return;
        }
        let now = b.now_ms();
        let actions = inner.lifecycle.activate(id, now);
        inner.active = Some(id);
        actions
    };
    // Internal pages: hide whatever was visible, the UI draws the page.
    apply_actions(b, actions);
    b.lifecycle_signal.notify();
    place_active(b);
    let (overlay, wv) = {
        let inner = b.inner.lock();
        (inner.overlay, inner.webview(id))
    };
    if let Some(wv) = wv {
        if overlay {
            let _ = wv.hide();
        } else if focus_page {
            let _ = wv.set_focus();
        }
    }
    b.emit("tab:activated", serde_json::json!({ "id": id }));
    emit_all_states(b);
    crate::session::schedule_save(b);
}

/// Emits every tab whose lifecycle state changed (cheap: only state fields).
pub fn emit_all_states(b: &Browser) {
    let states: Vec<_> = {
        let inner = b.inner.lock();
        inner.tabs.iter().filter_map(|t| inner.lifecycle.state(t.info.id).map(|s| (t.info.id, s))).collect()
    };
    b.emit("tab:states", states);
}

/// Carries out lifecycle actions. Safe to call from any thread except inside a
/// WebView2 callback when the action list contains `Recreate`.
pub fn apply_actions(b: &SharedBrowser, actions: Vec<Action>) {
    for action in actions {
        apply_action(b, action);
    }
}

fn apply_action(b: &SharedBrowser, action: Action) {
    let id = match action {
        Action::Show(id)
        | Action::Hide(id)
        | Action::MemoryLow(id)
        | Action::MemoryNormal(id)
        | Action::Suspend(id)
        | Action::Resume(id)
        | Action::Discard(id)
        | Action::Recreate(id) => id,
    };
    let (wv, url, internal) = {
        let inner = b.inner.lock();
        match inner.tab(id) {
            Some(t) => (t.webview.clone(), t.info.url.clone(), t.info.internal.is_some()),
            None => return,
        }
    };
    match action {
        Action::Show(_) => {
            if let Some(wv) = wv {
                if let Some(r) = content_rect(b) {
                    let _ = wv.set_bounds(r);
                }
                let overlay = b.inner.lock().overlay;
                if !overlay {
                    let _ = wv.show();
                }
            }
        }
        Action::Hide(_) => {
            if let Some(wv) = wv {
                let _ = wv.hide();
            }
        }
        Action::MemoryLow(_) | Action::MemoryNormal(_) => {
            if let Some(wv) = wv {
                let low = matches!(action, Action::MemoryLow(_));
                webview2::with_core(&wv, move |_, core| webview2::set_memory_low(&core, low));
            }
        }
        Action::Suspend(_) => {
            if let Some(wv) = wv {
                let b2 = Arc::downgrade(b);
                webview2::with_core(&wv, move |_, core| {
                    webview2::set_memory_low(&core, true);
                    webview2::try_suspend(&core, move |ok| {
                        if !ok && let Some(b) = b2.upgrade() {
                            let now = b.now_ms();
                            b.inner.lock().lifecycle.suspend_failed(id, now);
                            b.lifecycle_signal.notify();
                            b.emit_tab(id);
                        }
                    });
                });
            }
            b.emit_tab(id);
        }
        Action::Resume(_) => {
            if let Some(wv) = wv {
                webview2::with_core(&wv, move |_, core| webview2::resume(&core));
            }
        }
        Action::Discard(_) => discard(b, id, wv),
        Action::Recreate(_) => {
            if internal {
                return;
            }
            let b2 = b.clone();
            // Webview creation must not run inside a WebView2 callback.
            std::thread::spawn(move || match create_webview(&b2, id, &url, true) {
                Ok(wv) => {
                    let mut inner = b2.inner.lock();
                    let still_open = inner.tab(id).is_some();
                    let active = inner.active == Some(id);
                    if let Some(t) = inner.tab_mut(id) {
                        t.webview = Some(wv.clone());
                        t.info.crashed = false;
                    }
                    drop(inner);
                    if !still_open {
                        let _ = wv.close();
                    } else if !active {
                        let _ = wv.hide();
                    } else {
                        let _ = wv.set_focus();
                    }
                    b2.emit_tab(id);
                }
                Err(e) => log::warn!("recreate tab {id}: {e}"),
            });
        }
    }
}

/// Closes a tab's webview but keeps the tab (url, title, favicon, snapshot).
fn discard(b: &SharedBrowser, id: TabId, wv: Option<Webview<Wry>>) {
    let Some(wv) = wv else { return };
    let path = b.paths.snapshot(id);
    let b2 = b.clone();
    let wv2 = wv.clone();
    // Keep a snapshot first (JPEG), then close the webview.
    webview2::with_core(&wv, move |_, core| {
        webview2::capture_preview(&core, false, move |bytes| {
            if let Some(bytes) = bytes {
                let _ = std::fs::write(&path, bytes);
                if let Some(t) = b2.inner.lock().tab_mut(id) {
                    t.info.has_snapshot = true;
                }
            }
            if let Some(t) = b2.inner.lock().tab_mut(id) {
                t.webview = None;
                t.main_frame_id = None;
            }
            let _ = wv2.close();
            b2.emit_tab(id);
        });
    });
}

/// Closes a tab and picks the next one (right neighbor, else left).
pub async fn close(b: &SharedBrowser, id: TabId) {
    let (wv, next, was_active, last) = {
        let mut inner = b.inner.lock();
        let Some(index) = inner.index_of(id) else { return };
        let tab = inner.tabs.remove(index);
        inner.lifecycle.remove(id);
        if !tab.info.private && tab.info.internal != Some(InternalPage::NewTab) {
            let closed = SessionTab {
                url: tab.info.url.clone(),
                title: tab.info.title.clone(),
                favicon_url: None,
                pinned: tab.info.pinned,
                muted: tab.info.muted,
                zoom: tab.info.zoom,
                snapshot: None,
            };
            inner.closed.push((index, closed));
            if inner.closed.len() > MAX_CLOSED {
                inner.closed.remove(0);
            }
        }
        let was_active = inner.active == Some(id);
        if was_active {
            inner.active = None;
        }
        let next = if was_active {
            inner.tabs.get(index).or_else(|| inner.tabs.get(index.wrapping_sub(1))).map(|t| t.info.id)
        } else {
            None
        };
        (tab.webview, next, was_active, inner.tabs.is_empty())
    };
    if let Some(wv) = wv {
        let _ = wv.close();
    }
    let _ = std::fs::remove_file(b.paths.snapshot(id));
    b.emit("tab:closed", serde_json::json!({ "id": id }));
    // Private browsing data goes away with the last private tab.
    if last {
        let _ = create(b, None, CreateOptions::default()).await;
    } else if let (true, Some(n)) = (was_active, next) {
        activate(b, n, false).await;
    }
    b.lifecycle_signal.notify();
    crate::session::schedule_save(b);
}

pub async fn reopen_closed(b: &SharedBrowser) -> Option<TabId> {
    let (index, tab) = b.inner.lock().closed.pop()?;
    let opts = CreateOptions { index: Some(index), pinned: tab.pinned, ..Default::default() };
    create(b, Some(tab.url), opts).await.ok()
}

pub async fn duplicate(b: &SharedBrowser, id: TabId) -> Option<TabId> {
    let (url, private) = {
        let inner = b.inner.lock();
        let t = inner.tab(id)?;
        (t.info.url.clone(), t.info.private)
    };
    create(b, Some(url), CreateOptions { opener: Some(id), private, ..Default::default() }).await.ok()
}

pub fn move_tab(b: &SharedBrowser, id: TabId, to: usize) {
    let order: Vec<TabId> = {
        let mut inner = b.inner.lock();
        let Some(from) = inner.index_of(id) else { return };
        let tab = inner.tabs.remove(from);
        let pinned_count = inner.tabs.iter().filter(|t| t.info.pinned).count();
        let to = if tab.info.pinned { to.min(pinned_count) } else { to.clamp(pinned_count, inner.tabs.len()) };
        inner.tabs.insert(to, tab);
        inner.tabs.iter().map(|t| t.info.id).collect()
    };
    b.emit("tabs:order", order);
    crate::session::schedule_save(b);
}

pub fn set_pinned(b: &SharedBrowser, id: TabId, pinned: bool) {
    let flags = {
        let mut inner = b.inner.lock();
        let Some(t) = inner.tab_mut(id) else { return };
        t.info.pinned = pinned;
        t.flags.pinned = pinned;
        t.flags
    };
    // Pinned tabs sit at the start of the strip: pinning appends to that group,
    // unpinning puts the tab right after it.
    let pinned_count = b.inner.lock().tabs.iter().filter(|t| t.info.pinned && t.info.id != id).count();
    move_tab(b, id, pinned_count);
    let now = b.now_ms();
    let actions = b.inner.lock().lifecycle.set_flags(id, flags, now);
    apply_actions(b, actions);
    b.emit_tab(id);
}

pub fn set_muted(b: &SharedBrowser, id: TabId, muted: bool) {
    let wv = {
        let mut inner = b.inner.lock();
        let Some(t) = inner.tab_mut(id) else { return };
        t.info.muted = muted;
        t.webview.clone()
    };
    if let Some(wv) = wv {
        webview2::with_core(&wv, move |_, core| webview2::set_muted(&core, muted));
    }
    b.emit_tab(id);
}

/// Omnibox Enter.
pub async fn go(b: &SharedBrowser, id: TabId, input: &str, new_tab: bool) -> Result<(), String> {
    let dev = b.settings().developer_mode;
    let Some(dest) = omnibox::classify(input, ClassifyOptions { developer_mode: dev }) else { return Ok(()) };
    let url = match dest {
        Destination::Navigate { url } | Destination::Search { url, .. } => url,
        Destination::Internal { url, .. } => url,
        Destination::Blocked { reason } => {
            b.toast(match reason {
                omnibox::BlockReason::ScriptUrl => "JavaScript links can't be run from the address bar",
                omnibox::BlockReason::EnginePage => "That page isn't available in Limbo",
            });
            return Ok(());
        }
    };
    if new_tab {
        let private = b.inner.lock().tab(id).map(|t| t.info.private).unwrap_or(false);
        create(b, Some(url), CreateOptions { typed: true, private, ..Default::default() }).await?;
        return Ok(());
    }
    navigate(b, id, &url, true).await
}

/// Loads a URL in an existing tab (creating/closing its webview as needed).
pub async fn navigate(b: &SharedBrowser, id: TabId, url: &str, typed: bool) -> Result<(), String> {
    let internal = InternalPage::from_url(url).is_some();
    let (wv, active) = {
        let mut inner = b.inner.lock();
        let active = inner.active == Some(id);
        let Some(t) = inner.tab_mut(id) else { return Err("no such tab".into()) };
        t.typed_navigation = typed;
        if url.starts_with("file:") && typed {
            t.allow_file = true;
        }
        if internal || t.webview.is_none() {
            t.info.set_url(url);
            if !internal {
                t.info.loading = true;
                t.info.progress = 0.05;
                t.info.title.clear();
                t.info.favicon = None;
            }
        }
        (t.webview.clone(), active)
    };
    if internal {
        // Leave the web page: its webview goes away, the UI draws the page.
        if let Some(wv) = wv {
            let _ = wv.close();
            let now = b.now_ms();
            let mut inner = b.inner.lock();
            if let Some(t) = inner.tab_mut(id) {
                t.webview = None;
            }
            inner.lifecycle.remove(id);
            let actions = inner.lifecycle.add(id, TabFlags::default(), true, now);
            let actions2 = if active { inner.lifecycle.activate(id, now) } else { vec![] };
            drop(inner);
            apply_actions(b, actions);
            apply_actions(b, actions2);
        }
        b.emit_tab(id);
        return Ok(());
    }
    match wv {
        Some(wv) => {
            let url = url.to_string();
            webview2::with_core(&wv, move |_, core| webview2::navigate(&core, &url));
            if active && !b.inner.lock().overlay {
                let _ = wv.set_focus();
            }
        }
        None => {
            // New Tab page (or unloaded tab) becoming a web page.
            b.emit_tab(id);
            let wv = create_webview(b, id, url, active)?;
            let now = b.now_ms();
            let mut inner = b.inner.lock();
            if let Some(t) = inner.tab_mut(id) {
                t.webview = Some(wv.clone());
            }
            let flags = inner.tab(id).map(|t| t.flags).unwrap_or_default();
            inner.lifecycle.remove(id);
            let mut actions = inner.lifecycle.add(id, flags, false, now);
            if active {
                actions.extend(inner.lifecycle.activate(id, now));
            }
            drop(inner);
            apply_actions(b, actions);
            if active {
                place_active(b);
                let _ = wv.set_focus();
            }
        }
    }
    crate::session::schedule_save(b);
    Ok(())
}

pub fn nav_command(b: &SharedBrowser, id: TabId, cmd: &'static str) {
    let Some(wv) = b.inner.lock().webview(id) else { return };
    webview2::with_core(&wv, move |_, core| unsafe {
        let _ = match cmd {
            "back" => core.GoBack(),
            "forward" => core.GoForward(),
            "reload" => core.Reload(),
            "stop" => core.Stop(),
            _ => Ok(()),
        };
    });
}

/// Hard reload: bypass the cache.
pub fn hard_reload(b: &SharedBrowser, id: TabId) {
    let Some(wv) = b.inner.lock().webview(id) else { return };
    webview2::with_core(&wv, move |_, core| {
        webview2::execute_script(&core, "location.reload()");
        let _ = &core;
    });
}

/// Ctrl+Tab / Ctrl+1..9 helpers.
pub fn neighbor(b: &Browser, delta: isize) -> Option<TabId> {
    let inner = b.inner.lock();
    let n = inner.tabs.len() as isize;
    if n == 0 {
        return None;
    }
    let cur = inner.active.and_then(|a| inner.index_of(a)).unwrap_or(0) as isize;
    let i = (cur + delta).rem_euclid(n);
    Some(inner.tabs[i as usize].info.id)
}

pub fn nth(b: &Browser, n: usize) -> Option<TabId> {
    let inner = b.inner.lock();
    if n == 9 { inner.tabs.last() } else { inner.tabs.get(n.saturating_sub(1)) }.map(|t| t.info.id)
}
