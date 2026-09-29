//! Per-site zoom, remembered in the database, with a toast showing the level.

use limbo_core::sites;

use crate::state::{SharedBrowser, TabId};
use crate::webview2;

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Step {
    In,
    Out,
    Reset,
}

pub async fn step(b: &SharedBrowser, id: TabId, dir: Step) {
    let (current, url, wv, private) = {
        let inner = b.inner.lock();
        let Some(t) = inner.tab(id) else { return };
        (t.info.zoom, t.info.url.clone(), t.webview.clone(), t.info.private)
    };
    let default = b.settings().default_zoom;
    let next = match dir {
        Step::In => sites::step_zoom(current, true),
        Step::Out => sites::step_zoom(current, false),
        Step::Reset => default,
    };
    set(b, id, next, wv);
    if let (Some(host), false) = (crate::util::site_host(&url), private) {
        let _ = b.db.run(move |db| sites::set_zoom(db.conn(), &host, next, default)).await;
    }
    b.emit("zoom", serde_json::json!({ "id": id, "factor": next, "default": default }));
}

fn set(b: &SharedBrowser, id: TabId, factor: f64, wv: Option<tauri::Webview>) {
    if let Some(t) = b.inner.lock().tab_mut(id) {
        t.info.zoom = factor;
    }
    if let Some(wv) = wv {
        webview2::with_core(&wv, move |controller, _| webview2::set_zoom(&controller, factor));
    }
    b.emit_tab(id);
}

/// Applies the stored zoom for the page's site (on navigation and creation).
pub fn apply_saved(b: &SharedBrowser, id: TabId, url: &str) {
    let Some(host) = crate::util::site_host(url) else { return };
    let b2 = b.clone();
    let default = b.settings().default_zoom;
    b.db.spawn(move |db| {
        let factor = sites::zoom(db.conn(), &host).ok().flatten().unwrap_or(default);
        let (current, wv) = {
            let inner = b2.inner.lock();
            let Some(t) = inner.tab(id) else { return };
            (t.info.zoom, t.webview.clone())
        };
        if (current - factor).abs() > 0.001 {
            set(&b2, id, factor, wv);
        }
    });
}
