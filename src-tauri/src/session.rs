//! Saves the open tabs (debounced) and restores them on startup. Only the
//! active tab gets a webview at startup; the others come back unloaded.

use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use limbo_core::sessions::{Session, SessionTab, WindowPlacement};

use crate::state::{Browser, SharedBrowser};
use crate::tabs::{self, CreateOptions};

static SAVE_SCHEDULED: AtomicBool = AtomicBool::new(false);

/// Saves the session ~2 s after the last change.
pub fn schedule_save(b: &SharedBrowser) {
    if SAVE_SCHEDULED.swap(true, Ordering::AcqRel) {
        return;
    }
    let b = b.clone();
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(Duration::from_secs(2)).await;
        SAVE_SCHEDULED.store(false, Ordering::Release);
        save_now(&b);
    });
}

pub fn snapshot(b: &Browser) -> Session {
    let placement = window_placement(b);
    let inner = b.inner.lock();
    let tabs: Vec<SessionTab> = inner
        .tabs
        .iter()
        .filter(|t| !t.info.private)
        .map(|t| SessionTab {
            url: t.info.url.clone(),
            title: t.info.title.clone(),
            favicon_url: None,
            pinned: t.info.pinned,
            muted: t.info.muted,
            zoom: t.info.zoom,
            snapshot: t.info.has_snapshot.then(|| format!("{}.jpg", t.info.id)),
        })
        .collect();
    let active = inner
        .active
        .and_then(|a| inner.tabs.iter().filter(|t| !t.info.private).position(|t| t.info.id == a))
        .unwrap_or(0);
    Session { tabs, active, window: placement }
}

fn window_placement(b: &Browser) -> Option<WindowPlacement> {
    let maximized = b.window.is_maximized().ok()?;
    let pos = b.window.outer_position().ok()?;
    let size = b.window.inner_size().ok()?;
    if b.window.is_minimized().unwrap_or(false) {
        return None;
    }
    Some(WindowPlacement { x: pos.x, y: pos.y, width: size.width, height: size.height, maximized })
}

pub fn save_now(b: &Browser) {
    let session = snapshot(b);
    // Keep the last known window placement if the window is minimized right now.
    b.db.spawn(move |db| {
        let mut session = session;
        if session.window.is_none()
            && let Ok(Some(prev)) = limbo_core::sessions::load(db.conn())
        {
            session.window = prev.window;
        }
        if let Err(e) = limbo_core::sessions::save(db.conn(), &session, limbo_core::db::now_us()) {
            log::warn!("saving the session failed: {e}");
        }
    });
}

/// Restores tabs (or opens a New Tab). `extra` are tabs to add (Firefox import
/// on first run, URLs from the command line).
pub async fn restore(b: &SharedBrowser, session: Option<Session>, urls: Vec<String>) {
    let restore = b.settings().restore_tabs_on_startup;
    let session = session.filter(|_| restore).map(|s| s.normalized(200));
    let mut active_id = None;
    if let Some(s) = &session {
        for (i, t) in s.tabs.iter().enumerate() {
            let is_active = i == s.active && urls.is_empty();
            let id = add_restored(b, t, is_active);
            if is_active {
                active_id = Some(id);
            }
        }
    }
    for url in &urls {
        if let Ok(id) =
            tabs::create(b, Some(url.clone()), CreateOptions { background: true, ..Default::default() }).await
        {
            active_id = Some(id);
        }
    }
    match active_id {
        Some(id) => tabs::activate(b, id, false).await,
        None => {
            let _ = tabs::create(b, None, CreateOptions::default()).await;
        }
    }
}

/// Adds a tab without a webview; it loads when activated.
pub fn add_restored(b: &SharedBrowser, t: &SessionTab, _active: bool) -> u64 {
    let (id, info, index) = {
        let mut inner = b.inner.lock();
        let id = inner.alloc_id();
        let mut info = crate::state::TabInfo::new(id, &t.url, false);
        if info.internal.is_none() {
            info.title = t.title.clone();
        }
        info.pinned = t.pinned;
        info.muted = t.muted;
        info.zoom = t.zoom;
        // Carry the old snapshot over to the new id.
        if let Some(name) = &t.snapshot {
            let from = b.paths.snapshots.join(name);
            let to = b.paths.snapshot(id);
            if from != to && std::fs::rename(&from, &to).is_ok() {
                info.has_snapshot = true;
            }
        }
        let mut tab = crate::state::Tab::new(info.clone());
        tab.flags.pinned = t.pinned;
        inner.tabs.push(tab);
        let now = b.now_ms();
        let flags = limbo_core::lifecycle::TabFlags { pinned: t.pinned, ..Default::default() };
        let _ = inner.lifecycle.add(id, flags, true, now);
        (id, info, inner.tabs.len() - 1)
    };
    b.emit("tab:created", tabs::TabCreated { tab: info, index });
    // Favicon from the cache.
    let b2 = b.clone();
    let url = t.url.clone();
    b.db.spawn(move |db| {
        if let Ok(Some((mime, data))) = limbo_core::favicons::for_page(db.conn(), &url) {
            let data_url = crate::util::data_url(&mime, &data);
            if let Some(tab) = b2.inner.lock().tab_mut(id) {
                tab.info.favicon = Some(data_url);
            }
            b2.emit_tab(id);
        }
    });
    id
}
