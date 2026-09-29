//! Thin helpers over WebView2 COM (via `webview2-com`) for everything Tauri
//! doesn't wrap. Every function here must run on the UI thread; get there
//! with [`with_core`], which uses Tauri's `with_webview`.

use tauri::{Webview, Wry};
use tokio::sync::oneshot;
use webview2_com::Microsoft::Web::WebView2::Win32::*;
use webview2_com::*;
use windows::Win32::System::Com::{IStream, STATFLAG_NONAME, STREAM_SEEK_SET};
use windows::Win32::UI::Shell::SHCreateMemStream;
use windows_core::{HSTRING, Interface, PWSTR};

/// The shared `ICoreWebView2Environment`, reused by every tab webview so they
/// all live in one browser process. Only touched on the UI thread.
pub struct SendEnv(pub ICoreWebView2Environment);
// SAFETY: the environment is created and used on the UI thread only; Tauri
// moves it between threads inside `WebviewAttributes`, which it marks Send too.
unsafe impl Send for SendEnv {}
unsafe impl Sync for SendEnv {}

/// Runs `f` with the controller and core webview on the UI thread.
pub fn with_core(webview: &Webview<Wry>, f: impl FnOnce(ICoreWebView2Controller, ICoreWebView2) + Send + 'static) {
    let r = webview.with_webview(move |pw| {
        let controller = pw.controller();
        match unsafe { controller.CoreWebView2() } {
            Ok(core) => f(controller, core),
            Err(e) => log::warn!("CoreWebView2 unavailable: {e}"),
        }
    });
    if let Err(e) = r {
        log::warn!("with_webview: {e}");
    }
}

/// Like [`with_core`] but returns a value (awaitable from async commands).
pub async fn with_core_result<T: Send + 'static>(
    webview: &Webview<Wry>,
    f: impl FnOnce(ICoreWebView2Controller, ICoreWebView2) -> T + Send + 'static,
) -> Option<T> {
    let (tx, rx) = oneshot::channel();
    with_core(webview, move |c, w| {
        let _ = tx.send(f(c, w));
    });
    rx.await.ok()
}

/// For COM calls whose completion arrives in a callback: `f` receives a
/// sender to call when done.
pub async fn with_core_async<T: Send + 'static>(
    webview: &Webview<Wry>,
    f: impl FnOnce(ICoreWebView2Controller, ICoreWebView2, oneshot::Sender<T>) + Send + 'static,
) -> Option<T> {
    let (tx, rx) = oneshot::channel();
    with_core(webview, move |c, w| f(c, w, tx));
    rx.await.ok()
}

pub fn take_string(p: PWSTR) -> String {
    webview2_com::take_pwstr(p)
}

pub fn source(core: &ICoreWebView2) -> String {
    let mut p = PWSTR::null();
    unsafe {
        let _ = core.Source(&mut p);
    }
    take_string(p)
}

pub fn title(core: &ICoreWebView2) -> String {
    let mut p = PWSTR::null();
    unsafe {
        let _ = core.DocumentTitle(&mut p);
    }
    take_string(p)
}

pub fn history_state(core: &ICoreWebView2) -> (bool, bool) {
    let mut back = windows_core::BOOL::default();
    let mut fwd = windows_core::BOOL::default();
    unsafe {
        let _ = core.CanGoBack(&mut back);
        let _ = core.CanGoForward(&mut fwd);
    }
    (back.as_bool(), fwd.as_bool())
}

/// Reads an entire `IStream` from the start.
pub fn stream_bytes(stream: &IStream) -> Vec<u8> {
    unsafe {
        let mut stat = Default::default();
        if stream.Stat(&mut stat, STATFLAG_NONAME).is_err() {
            return Vec::new();
        }
        let size = stat.cbSize as usize;
        let _ = stream.Seek(0, STREAM_SEEK_SET, None);
        let mut buf = vec![0u8; size];
        let mut read = 0u32;
        let mut total = 0usize;
        while total < size {
            let hr = stream.Read(buf[total..].as_mut_ptr() as *mut _, (size - total) as u32, Some(&mut read));
            if hr.is_err() || read == 0 {
                break;
            }
            total += read as usize;
        }
        buf.truncate(total);
        buf
    }
}

pub fn set_memory_low(core: &ICoreWebView2, low: bool) {
    if let Ok(c19) = core.cast::<ICoreWebView2_19>() {
        let level = if low {
            COREWEBVIEW2_MEMORY_USAGE_TARGET_LEVEL_LOW
        } else {
            COREWEBVIEW2_MEMORY_USAGE_TARGET_LEVEL_NORMAL
        };
        unsafe {
            let _ = c19.SetMemoryUsageTargetLevel(level);
        }
    }
}

/// `TrySuspend`; `done(true)` when the renderer was frozen.
pub fn try_suspend(core: &ICoreWebView2, done: impl FnOnce(bool) + 'static) {
    let Ok(c3) = core.cast::<ICoreWebView2_3>() else {
        done(false);
        return;
    };
    let done = std::cell::RefCell::new(Some(done));
    let handler = TrySuspendCompletedHandler::create(Box::new(move |hr, ok| {
        if let Some(d) = done.borrow_mut().take() {
            d(hr.is_ok() && ok);
        }
        Ok(())
    }));
    unsafe {
        if c3.TrySuspend(&handler).is_err() {
            log::debug!("TrySuspend call failed");
        }
    }
}

pub fn resume(core: &ICoreWebView2) {
    if let Ok(c3) = core.cast::<ICoreWebView2_3>() {
        unsafe {
            let _ = c3.Resume();
        }
    }
}

/// Captures the visible page as JPEG (fast) or PNG.
pub fn capture_preview(core: &ICoreWebView2, png: bool, done: impl FnOnce(Option<Vec<u8>>) + 'static) {
    let Some(stream) = (unsafe { SHCreateMemStream(None) }) else {
        done(None);
        return;
    };
    let format = if png {
        COREWEBVIEW2_CAPTURE_PREVIEW_IMAGE_FORMAT_PNG
    } else {
        COREWEBVIEW2_CAPTURE_PREVIEW_IMAGE_FORMAT_JPEG
    };
    let s2 = stream.clone();
    let done = std::cell::RefCell::new(Some(done));
    let handler = CapturePreviewCompletedHandler::create(Box::new(move |hr| {
        if let Some(d) = done.borrow_mut().take() {
            d(hr.ok().map(|_| stream_bytes(&s2)).filter(|b| !b.is_empty()));
        }
        Ok(())
    }));
    unsafe {
        if core.CapturePreview(format, &stream, &handler).is_err() {
            log::debug!("CapturePreview failed");
        }
    }
}

/// The page favicon as PNG bytes.
pub fn get_favicon(core: &ICoreWebView2, done: impl FnOnce(Option<Vec<u8>>) + 'static) {
    let Ok(c15) = core.cast::<ICoreWebView2_15>() else {
        done(None);
        return;
    };
    let done = std::cell::RefCell::new(Some(done));
    let handler = GetFaviconCompletedHandler::create(Box::new(move |hr, stream| {
        let bytes = if hr.is_ok() { stream.map(|s| stream_bytes(&s)).filter(|b| !b.is_empty()) } else { None };
        if let Some(d) = done.borrow_mut().take() {
            d(bytes);
        }
        Ok(())
    }));
    unsafe {
        let _ = c15.GetFavicon(COREWEBVIEW2_FAVICON_IMAGE_FORMAT_PNG, &handler);
    }
}

pub fn favicon_uri(core: &ICoreWebView2) -> Option<String> {
    let c15 = core.cast::<ICoreWebView2_15>().ok()?;
    let mut p = PWSTR::null();
    unsafe { c15.FaviconUri(&mut p).ok()? };
    Some(take_string(p)).filter(|s| !s.is_empty())
}

pub fn set_muted(core: &ICoreWebView2, muted: bool) {
    if let Ok(c8) = core.cast::<ICoreWebView2_8>() {
        unsafe {
            let _ = c8.SetIsMuted(muted);
        }
    }
}

pub fn is_playing_audio(core: &ICoreWebView2) -> bool {
    let Ok(c8) = core.cast::<ICoreWebView2_8>() else { return false };
    let mut v = windows_core::BOOL::default();
    unsafe {
        let _ = c8.IsDocumentPlayingAudio(&mut v);
    }
    v.as_bool()
}

pub fn main_frame_id(core: &ICoreWebView2) -> Option<u32> {
    let c20 = core.cast::<ICoreWebView2_20>().ok()?;
    let mut id = 0u32;
    unsafe { c20.FrameId(&mut id).ok()? };
    Some(id)
}

pub fn profile(core: &ICoreWebView2) -> Option<ICoreWebView2Profile> {
    let c13 = core.cast::<ICoreWebView2_13>().ok()?;
    unsafe { c13.Profile().ok() }
}

pub fn navigate(core: &ICoreWebView2, url: &str) {
    unsafe {
        if let Err(e) = core.Navigate(&HSTRING::from(url)) {
            log::warn!("Navigate failed: {e}");
        }
    }
}

pub fn execute_script(core: &ICoreWebView2, js: &str) {
    let handler = ExecuteScriptCompletedHandler::create(Box::new(|_, _| Ok(())));
    unsafe {
        let _ = core.ExecuteScript(&HSTRING::from(js), &handler);
    }
}

/// Per-webview settings shared by every tab.
pub fn apply_tab_settings(core: &ICoreWebView2, smartscreen: bool, devtools: bool) {
    unsafe {
        let Ok(settings) = core.Settings() else { return };
        let _ = settings.SetIsStatusBarEnabled(false);
        let _ = settings.SetAreDefaultContextMenusEnabled(true);
        let _ = settings.SetAreDevToolsEnabled(devtools);
        let _ = settings.SetIsZoomControlEnabled(false);
        if let Ok(s3) = settings.cast::<ICoreWebView2Settings3>() {
            // Our own shortcuts (AcceleratorKeyPressed) replace the browser ones.
            let _ = s3.SetAreBrowserAcceleratorKeysEnabled(true);
        }
        if let Ok(s4) = settings.cast::<ICoreWebView2Settings4>() {
            // Limbo's vault handles passwords; avoid two save prompts.
            let _ = s4.SetIsPasswordAutosaveEnabled(false);
            let _ = s4.SetIsGeneralAutofillEnabled(false);
        }
        if let Ok(s8) = settings.cast::<ICoreWebView2Settings8>() {
            let _ = s8.SetIsReputationCheckingRequired(smartscreen);
        }
    }
}

/// Profile-wide settings (once, on the UI webview's profile, shared by all tabs).
pub fn apply_profile_settings(core: &ICoreWebView2) {
    let Some(profile) = profile(core) else { return };
    unsafe {
        if let Ok(p3) = profile.cast::<ICoreWebView2Profile3>() {
            let _ = p3.SetPreferredTrackingPreventionLevel(COREWEBVIEW2_TRACKING_PREVENTION_LEVEL_BALANCED);
        }
        if let Ok(p6) = profile.cast::<ICoreWebView2Profile6>() {
            let _ = p6.SetIsPasswordAutosaveEnabled(false);
            let _ = p6.SetIsGeneralAutofillEnabled(false);
        }
    }
}

pub fn set_zoom(controller: &ICoreWebView2Controller, factor: f64) {
    unsafe {
        let _ = controller.SetZoomFactor(factor);
    }
}
