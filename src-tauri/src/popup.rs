//! Real popup windows (`window.open` with a size), used by OAuth flows such as
//! "Sign in with Google". They share the opener's environment so
//! `window.opener` keeps working, and close themselves when the flow ends.

use std::sync::atomic::{AtomicU64, Ordering};

use tauri::webview::NewWindowFeatures;
use tauri::{WebviewUrl, WebviewWindow, WebviewWindowBuilder, Wry};

use crate::state::{Browser, TabId};

static NEXT: AtomicU64 = AtomicU64::new(1);

pub fn open(
    b: &Browser,
    opener: TabId,
    url: &url::Url,
    features: NewWindowFeatures,
) -> Result<WebviewWindow<Wry>, String> {
    let (settings, private) = {
        let inner = b.inner.lock();
        (inner.settings.clone(), inner.tab(opener).map(|t| t.info.private).unwrap_or(false))
    };
    let label = format!("popup-{}", NEXT.fetch_add(1, Ordering::Relaxed));
    let title = url.host_str().unwrap_or("Limbo").to_string();
    let has_position = features.position().is_some();
    let mut builder = WebviewWindowBuilder::new(&b.app, &label, WebviewUrl::External(url.clone()))
        .title(&title)
        .data_directory(b.paths.profile.clone())
        .additional_browser_args(&settings.browser_args())
        .browser_extensions_enabled(true)
        .incognito(private)
        .min_inner_size(320.0, 240.0)
        .resizable(true)
        .initialization_script(crate::passwords::AUTOFILL_JS)
        .on_document_title_changed(|w, t| {
            let _ = w.set_title(&t);
        })
        .window_features(features);
    if !has_position {
        builder = builder.center();
    }
    builder.build().map_err(|e| e.to_string())
}
