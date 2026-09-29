//! Crash recovery. A crashed page shows "This page crashed. Reload" (UI).
//! If the whole WebView2 browser process dies, every webview (the UI too) is
//! gone; the simplest reliable recovery is to save the session and restart:
//! tabs come back unloaded, exactly like a normal startup.

use std::sync::atomic::{AtomicBool, Ordering};

use crate::state::Browser;

static RESTARTING: AtomicBool = AtomicBool::new(false);

pub fn browser_process_exited(b: &Browser) {
    if RESTARTING.swap(true, Ordering::AcqRel) {
        return;
    }
    log::error!("WebView2 browser process exited; restarting Limbo");
    save_and_restart(b);
}

/// Restart on request (settings that only apply to a new WebView2 environment).
pub fn restart(b: &Browser) {
    if RESTARTING.swap(true, Ordering::AcqRel) {
        return;
    }
    log::info!("restarting Limbo to apply settings");
    save_and_restart(b);
}

fn save_and_restart(b: &Browser) {
    let session = crate::session::snapshot(b);
    let _ = b.db.call_blocking(move |db| limbo_core::sessions::save(db.conn(), &session, limbo_core::db::now_us()));
    let app = b.app.clone();
    std::thread::spawn(move || {
        app.request_restart();
    });
}
