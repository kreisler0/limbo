//! Engine-side browsing data (cookies, cache, site storage) and printing.

use tokio::sync::oneshot;
use webview2_com::ClearBrowsingDataCompletedHandler;
use webview2_com::Microsoft::Web::WebView2::Win32::*;
use windows_core::Interface;

use crate::ipc::ClearKinds;
use crate::state::{SharedBrowser, TabId};
use crate::webview2;

pub async fn clear_engine(b: &SharedBrowser, since_us: i64, kinds: &ClearKinds) {
    let Some(ui) = b.ui.get().cloned() else { return };
    let mut mask = COREWEBVIEW2_BROWSING_DATA_KINDS(0);
    if kinds.cookies {
        mask.0 |= COREWEBVIEW2_BROWSING_DATA_KINDS_COOKIES.0 | COREWEBVIEW2_BROWSING_DATA_KINDS_ALL_DOM_STORAGE.0;
    }
    if kinds.cache {
        mask.0 |= COREWEBVIEW2_BROWSING_DATA_KINDS_DISK_CACHE.0 | COREWEBVIEW2_BROWSING_DATA_KINDS_CACHE_STORAGE.0;
    }
    if kinds.history {
        mask.0 |=
            COREWEBVIEW2_BROWSING_DATA_KINDS_BROWSING_HISTORY.0 | COREWEBVIEW2_BROWSING_DATA_KINDS_DOWNLOAD_HISTORY.0;
    }
    if kinds.autofill {
        mask.0 |= COREWEBVIEW2_BROWSING_DATA_KINDS_GENERAL_AUTOFILL.0;
    }
    if mask.0 == 0 {
        return;
    }
    let start = since_us as f64 / 1_000_000.0;
    let end = limbo_core::db::now_us() as f64 / 1_000_000.0 + 60.0;
    let _ = webview2::with_core_async(&ui, move |_, core, tx: oneshot::Sender<()>| {
        let Some(p2) = webview2::profile(&core).and_then(|p| p.cast::<ICoreWebView2Profile2>().ok()) else {
            let _ = tx.send(());
            return;
        };
        let tx = std::cell::RefCell::new(Some(tx));
        let handler = ClearBrowsingDataCompletedHandler::create(Box::new(move |_| {
            if let Some(tx) = tx.borrow_mut().take() {
                let _ = tx.send(());
            }
            Ok(())
        }));
        unsafe {
            let _ = p2.ClearBrowsingDataInTimeRange(mask, start, end, &handler);
        }
    })
    .await;
}

pub fn print(b: &SharedBrowser, id: TabId) {
    let Some(wv) = b.inner.lock().webview(id) else { return };
    webview2::with_core(&wv, |_, core| {
        if let Ok(c16) = core.cast::<ICoreWebView2_16>() {
            unsafe {
                let _ = c16.ShowPrintUI(COREWEBVIEW2_PRINT_DIALOG_KIND_BROWSER);
            }
        }
    });
}
