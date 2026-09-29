//! Find in page with WebView2's Find API (runtime 1.0.2849+), results shown in
//! Limbo's FindBar. Older runtimes fall back to `window.find` (no counts).

use std::sync::Weak;

use serde::Serialize;
use webview2_com::Microsoft::Web::WebView2::Win32::*;
use webview2_com::{
    FindActiveMatchIndexChangedEventHandler, FindMatchCountChangedEventHandler, FindStartCompletedHandler,
};
use windows_core::{HSTRING, Interface};

use crate::state::{Browser, SharedBrowser, TabId};
use crate::webview2;

#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FindResult {
    pub tab_id: TabId,
    pub matches: i32,
    /// 1-based; 0 when nothing is selected.
    pub active: i32,
}

fn emit_counts(b: &Browser, id: TabId, find: &ICoreWebView2Find) {
    let (mut count, mut index) = (0i32, -1i32);
    unsafe {
        let _ = find.MatchCount(&mut count);
        let _ = find.ActiveMatchIndex(&mut index);
    }
    b.emit("find:result", FindResult { tab_id: id, matches: count, active: (index + 1).max(0) });
}

pub fn wire(weak: &Weak<Browser>, id: TabId, core: &ICoreWebView2) -> windows_core::Result<()> {
    let Ok(c28) = core.cast::<ICoreWebView2_28>() else { return Ok(()) };
    let Ok(find) = (unsafe { c28.Find() }) else { return Ok(()) };
    let mut token = 0i64;
    unsafe {
        let w = weak.clone();
        find.add_MatchCountChanged(
            &FindMatchCountChangedEventHandler::create(Box::new(move |f, _| {
                if let (Some(b), Some(f)) = (w.upgrade(), f) {
                    emit_counts(&b, id, &f);
                }
                Ok(())
            })),
            &mut token,
        )?;
        let w = weak.clone();
        find.add_ActiveMatchIndexChanged(
            &FindActiveMatchIndexChangedEventHandler::create(Box::new(move |f, _| {
                if let (Some(b), Some(f)) = (w.upgrade(), f) {
                    emit_counts(&b, id, &f);
                }
                Ok(())
            })),
            &mut token,
        )?;
    }
    Ok(())
}

pub fn start(b: &SharedBrowser, id: TabId, text: String, match_case: bool) {
    let Some(wv) = b.inner.lock().webview(id) else { return };
    let b2 = b.clone();
    webview2::with_core(&wv, move |_, core| unsafe {
        let native = (|| -> windows_core::Result<()> {
            let find = core.cast::<ICoreWebView2_28>()?.Find()?;
            let env = b2.env.get().ok_or_else(|| windows_core::Error::from(windows::Win32::Foundation::E_FAIL))?;
            let options = env.0.cast::<ICoreWebView2Environment15>()?.CreateFindOptions()?;
            options.SetFindTerm(&HSTRING::from(text.as_str()))?;
            options.SetIsCaseSensitive(match_case)?;
            options.SetShouldHighlightAllMatches(true)?;
            options.SetSuppressDefaultFindDialog(true)?;
            let f2 = find.clone();
            let b3 = b2.clone();
            find.Start(
                &options,
                &FindStartCompletedHandler::create(Box::new(move |_| {
                    emit_counts(&b3, id, &f2);
                    Ok(())
                })),
            )
        })();
        if native.is_err() {
            let js = format!(
                "window.find({}, {}, false, true)",
                serde_json::to_string(&text).unwrap_or_default(),
                match_case
            );
            webview2::execute_script(&core, &js);
        }
    });
}

pub fn step(b: &SharedBrowser, id: TabId, forward: bool) {
    let Some(wv) = b.inner.lock().webview(id) else { return };
    webview2::with_core(&wv, move |_, core| unsafe {
        match core.cast::<ICoreWebView2_28>().and_then(|c| c.Find()) {
            Ok(find) => {
                let _ = if forward { find.FindNext() } else { find.FindPrevious() };
            }
            Err(_) => webview2::execute_script(
                &core,
                if forward { "window.find(undefined)" } else { "window.find(undefined,false,true)" },
            ),
        }
    });
}

pub fn stop(b: &SharedBrowser, id: TabId) {
    let Some(wv) = b.inner.lock().webview(id) else { return };
    webview2::with_core(&wv, move |_, core| unsafe {
        if let Ok(find) = core.cast::<ICoreWebView2_28>().and_then(|c| c.Find()) {
            let _ = find.Stop();
        }
    });
}
