//! The snapshot technique (plan 5.2). HTML in the UI webview can't draw over a
//! tab webview, so before a menu/palette/dialog opens over the page:
//! 1. `capture` returns a JPEG of the page (UI shows it exactly where the page is),
//! 2. `hide_content` hides the live webview once the image is on screen,
//! 3. `show_content` brings the page back when the overlay closes.

use crate::state::SharedBrowser;
use crate::webview2;

pub async fn capture(b: &SharedBrowser) -> Vec<u8> {
    let wv = b.inner.lock().active_webview();
    let Some(wv) = wv else { return Vec::new() };
    webview2::with_core_async(&wv, |_, core, tx| {
        webview2::capture_preview(&core, false, move |bytes| {
            let _ = tx.send(bytes.unwrap_or_default());
        })
    })
    .await
    .unwrap_or_default()
}

pub fn hide_content(b: &SharedBrowser) {
    let wv = {
        let mut inner = b.inner.lock();
        inner.overlay = true;
        inner.active_webview()
    };
    crate::extensions::close_popup(b);
    if let Some(wv) = wv {
        let _ = wv.hide();
    }
}

pub fn show_content(b: &SharedBrowser) {
    let wv = {
        let mut inner = b.inner.lock();
        inner.overlay = false;
        let crashed = inner.active.and_then(|a| inner.tab(a)).map(|t| t.info.crashed).unwrap_or(false);
        if crashed { None } else { inner.active_webview() }
    };
    if let Some(wv) = wv {
        if let Some(r) = crate::tabs::content_rect(b) {
            let _ = wv.set_bounds(r);
        }
        let _ = wv.show();
    }
}

/// Snapshot of a (possibly unloaded) tab: live capture if it has a webview,
/// else the JPEG kept when it was discarded.
pub async fn tab_snapshot(b: &SharedBrowser, id: u64) -> Vec<u8> {
    let wv = b.inner.lock().webview(id);
    if let Some(wv) = wv {
        let bytes = webview2::with_core_async(&wv, |_, core, tx| {
            webview2::capture_preview(&core, false, move |bytes| {
                let _ = tx.send(bytes.unwrap_or_default());
            })
        })
        .await
        .unwrap_or_default();
        if !bytes.is_empty() {
            return bytes;
        }
    }
    std::fs::read(b.paths.snapshot(id)).unwrap_or_default()
}
