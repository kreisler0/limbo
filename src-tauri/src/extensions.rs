//! Chrome extensions on WebView2: install from the Chrome Web Store / Edge
//! Add-ons (our own "Add to Limbo" flow, since the stores' buttons can't talk
//! to WebView2), from files, or unpacked; toolbar popups; options; updates.
//!
//! Installed folders are immutable: `Extensions\<id>\<version>\`. The CRX's
//! public key is written into the manifest before registering, so the ID stays
//! the store ID (and extension data survives updates).

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use base64::Engine;
use limbo_core::extensions::manifest::{self, Compatibility, ManifestInfo};
use limbo_core::extensions::registry::{self, InstalledExtension};
use limbo_core::extensions::store::Store;
use limbo_core::extensions::{self as ext, crx, update};
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use tauri::webview::WebviewBuilder;
use tauri::{LogicalPosition, LogicalSize, Manager, WebviewUrl};
use tokio::sync::oneshot;
use webview2_com::Microsoft::Web::WebView2::Win32::*;
use webview2_com::{
    BrowserExtensionEnableCompletedHandler, BrowserExtensionRemoveCompletedHandler, FocusChangedEventHandler,
    ProfileAddBrowserExtensionCompletedHandler, ProfileGetBrowserExtensionsCompletedHandler,
};
use windows_core::{BOOL, HSTRING, Interface, PWSTR};

use crate::state::SharedBrowser;
use crate::webview2::{self, take_string};

/// The engine's Chromium version, sent to the stores as `prodversion`.
pub static BROWSER_VERSION: OnceLock<String> = OnceLock::new();

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ExtensionInfo {
    pub id: String,
    pub name: String,
    pub version: String,
    pub description: String,
    pub enabled: bool,
    pub pinned: bool,
    pub source: String,
    pub icon: Option<String>,
    pub has_action: bool,
    pub popup: Option<String>,
    pub action_title: Option<String>,
    pub options_page: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InstallReview {
    pub token: u64,
    pub id: Option<String>,
    pub name: String,
    pub version: String,
    pub description: String,
    pub icon: Option<String>,
    pub warnings: Vec<String>,
    pub compatibility: Compatibility,
    pub source: String,
}

struct Staged {
    archive: Vec<u8>,
    id: Option<String>,
    public_key: Option<Vec<u8>>,
    manifest: ManifestInfo,
    source: String,
}

static STAGED: Mutex<Option<HashMap<u64, Staged>>> = Mutex::new(None);
static NEXT_TOKEN: AtomicU64 = AtomicU64::new(1);

fn icon_from_zip(archive: &[u8], m: &ManifestInfo) -> Option<String> {
    use std::io::Read;
    let path = m.best_icon(48)?.trim_start_matches('/').to_string();
    let mut zip = zip::ZipArchive::new(std::io::Cursor::new(archive)).ok()?;
    let mut f = zip.by_name(&path).ok()?;
    let mut buf = Vec::new();
    f.read_to_end(&mut buf).ok()?;
    Some(crate::util::data_url(limbo_core::favicons::sniff_mime(&buf), &buf))
}

fn icon_from_dir(dir: &Path, m: &ManifestInfo, size: u32) -> Option<String> {
    let rel = m.best_icon(size)?.trim_start_matches('/');
    let bytes = std::fs::read(dir.join(rel)).ok()?;
    Some(crate::util::data_url(limbo_core::favicons::sniff_mime(&bytes), &bytes))
}

fn stage(s: Staged) -> InstallReview {
    let token = NEXT_TOKEN.fetch_add(1, Ordering::Relaxed);
    let review = InstallReview {
        token,
        id: s.id.clone(),
        name: s.manifest.name.clone(),
        version: s.manifest.version.clone(),
        description: s.manifest.description.clone(),
        icon: icon_from_zip(&s.archive, &s.manifest),
        warnings: s.manifest.permission_warnings(),
        compatibility: s.manifest.compatibility(),
        source: s.source.clone(),
    };
    STAGED.lock().get_or_insert_with(HashMap::new).insert(token, s);
    review
}

fn prodversion() -> String {
    BROWSER_VERSION.get().cloned().unwrap_or_else(|| "140.0.0.0".into())
}

/// Downloads and verifies a store package, returning what the review dialog shows.
pub async fn prepare_store(b: &SharedBrowser, store: Store, id: String) -> Result<InstallReview, String> {
    if !ext::is_valid_id(&id) {
        return Err("that isn't an extension ID".into());
    }
    let agent = b.http.clone();
    let url = store.download_url(&id, &prodversion());
    let staged = tauri::async_runtime::spawn_blocking(move || -> Result<Staged, String> {
        let bytes = crate::http::get_bytes(&agent, &url, 256 * 1024 * 1024)?;
        let verified = crx::verify(&bytes, Some(&id)).map_err(|e| e.to_string())?;
        let manifest = manifest::read_zip(verified.archive).map_err(|e| e.to_string())?;
        Ok(Staged {
            archive: verified.archive.to_vec(),
            id: Some(verified.id),
            public_key: Some(verified.public_key),
            manifest,
            source: store.as_str().to_string(),
        })
    })
    .await
    .map_err(|e| e.to_string())??;
    Ok(stage(staged))
}

/// "Install from .crx/.zip/.xpi file" (developer toggle).
pub async fn prepare_file(b: &SharedBrowser) -> Result<Option<InstallReview>, String> {
    let Some(path) = crate::dialog::open_file(b, "Install an extension", &[("Extensions", "*.crx;*.zip;*.xpi")]).await
    else {
        return Ok(None);
    };
    let bytes = std::fs::read(&path).map_err(|e| e.to_string())?;
    let staged = if bytes.starts_with(b"Cr24") {
        let v = crx::verify(&bytes, None).map_err(|e| e.to_string())?;
        Staged {
            archive: v.archive.to_vec(),
            id: Some(v.id),
            public_key: Some(v.public_key),
            manifest: manifest::read_zip(v.archive).map_err(|e| e.to_string())?,
            source: "file".into(),
        }
    } else {
        let manifest = manifest::read_zip(&bytes).map_err(|e| e.to_string())?;
        Staged { archive: bytes, id: None, public_key: None, manifest, source: "file".into() }
    };
    Ok(Some(stage(staged)))
}

/// Adds `key` to manifest.json so WebView2 derives the store ID.
fn inject_key(dir: &Path, public_key: &[u8]) -> Result<(), String> {
    let path = dir.join("manifest.json");
    let text = std::fs::read_to_string(&path).map_err(|e| e.to_string())?;
    let mut v: serde_json::Value =
        serde_json::from_str(&manifest::strip_json_comments(&text)).map_err(|e| e.to_string())?;
    if let serde_json::Value::Object(m) = &mut v {
        m.insert("key".into(), serde_json::Value::String(base64::engine::general_purpose::STANDARD.encode(public_key)));
    }
    std::fs::write(&path, serde_json::to_vec_pretty(&v).map_err(|e| e.to_string())?).map_err(|e| e.to_string())
}

/// Registers a folder with the WebView2 profile; returns the extension ID.
async fn add_to_profile(b: &SharedBrowser, dir: PathBuf) -> Result<String, String> {
    let ui = b.ui.get().cloned().ok_or("UI not ready")?;
    webview2::with_core_async(&ui, move |_, core, tx: oneshot::Sender<Result<String, String>>| {
        let Some(p7) = webview2::profile(&core).and_then(|p| p.cast::<ICoreWebView2Profile7>().ok()) else {
            let _ = tx.send(Err("this WebView2 runtime doesn't support extensions".into()));
            return;
        };
        let tx = std::cell::RefCell::new(Some(tx));
        let handler = ProfileAddBrowserExtensionCompletedHandler::create(Box::new(move |hr, ext| {
            let result = match (hr, ext) {
                (Ok(()), Some(e)) => unsafe {
                    let mut p = PWSTR::null();
                    let _ = e.Id(&mut p);
                    Ok(take_string(p))
                },
                (Err(e), _) => Err(format!("WebView2 refused the extension: {e}")),
                _ => Err("WebView2 returned no extension".into()),
            };
            if let Some(tx) = tx.borrow_mut().take() {
                let _ = tx.send(result);
            }
            Ok(())
        }));
        unsafe {
            if let Err(e) = p7.AddBrowserExtension(&HSTRING::from(dir.as_os_str()), &handler) {
                log::warn!("AddBrowserExtension: {e}");
            }
        }
    })
    .await
    .ok_or("the UI thread didn't answer")?
}

pub async fn confirm(b: &SharedBrowser, token: u64) -> Result<ExtensionInfo, String> {
    let staged = STAGED.lock().as_mut().and_then(|m| m.remove(&token)).ok_or("this install expired; try again")?;
    if staged.manifest.compatibility() == Compatibility::FirefoxOnly {
        return Err("This add-on only works in Firefox.".into());
    }
    let folder_id = staged.id.clone().unwrap_or_else(|| {
        use sha2_shim::short_hash;
        format!("local-{}", short_hash(&staged.archive))
    });
    let dir = ext::install_dir(&b.paths.extensions, &folder_id, &staged.manifest.version);
    if !dir.exists() {
        let archive = staged.archive;
        let d = dir.clone();
        let key = staged.public_key.clone();
        tauri::async_runtime::spawn_blocking(move || -> Result<(), String> {
            ext::unpack(&archive, &d).map_err(|e| e.to_string())?;
            if let Some(k) = key {
                inject_key(&d, &k)?;
            }
            Ok(())
        })
        .await
        .map_err(|e| e.to_string())??;
    }
    let id = add_to_profile(b, dir.clone()).await?;
    let root = b.paths.extensions.clone();
    let _ = ext::remove_old_versions(&root, &folder_id, &dir);
    let record = InstalledExtension {
        id: id.clone(),
        name: staged.manifest.name.clone(),
        version: staged.manifest.version.clone(),
        path: dir.to_string_lossy().into_owned(),
        source: staged.source.clone(),
        installed_us: limbo_core::db::now_us(),
        updated_us: None,
        pinned: staged.manifest.has_action,
        position: 0,
    };
    let rec = record.clone();
    b.db.run(move |db| {
        let mut rec = rec;
        if let Some(existing) = registry::get(db.conn(), &rec.id)? {
            rec.pinned = existing.pinned;
            rec.position = existing.position;
            rec.installed_us = existing.installed_us;
            rec.updated_us = Some(limbo_core::db::now_us());
        } else {
            rec.position = registry::next_position(db.conn())?;
        }
        registry::upsert(db.conn(), &rec)
    })
    .await?;
    b.emit("ext:changed", ());
    Ok(info_for(&record, &staged.manifest, true))
}

mod sha2_shim {
    pub fn short_hash(data: &[u8]) -> String {
        // FNV-1a is plenty for naming a local folder.
        let mut h: u64 = 0xcbf29ce484222325;
        for b in data {
            h ^= *b as u64;
            h = h.wrapping_mul(0x100000001b3);
        }
        format!("{h:016x}")
    }
}

/// "Load unpacked" (developer mode): registers a folder in place.
pub async fn load_unpacked(b: &SharedBrowser) -> Result<Option<ExtensionInfo>, String> {
    let Some(dir) = crate::dialog::pick_folder(b, "Load an unpacked extension").await else { return Ok(None) };
    let m = manifest::read_dir(&dir).map_err(|e| e.to_string())?;
    if m.compatibility() == Compatibility::FirefoxOnly {
        return Err("This add-on only works in Firefox.".into());
    }
    let id = add_to_profile(b, dir.clone()).await?;
    let record = InstalledExtension {
        id,
        name: m.name.clone(),
        version: m.version.clone(),
        path: dir.to_string_lossy().into_owned(),
        source: "unpacked".into(),
        installed_us: limbo_core::db::now_us(),
        updated_us: None,
        pinned: m.has_action,
        position: 0,
    };
    let rec = record.clone();
    b.db.run(move |db| {
        let mut rec = rec;
        rec.position = registry::next_position(db.conn())?;
        registry::upsert(db.conn(), &rec)
    })
    .await?;
    b.emit("ext:changed", ());
    Ok(Some(info_for(&record, &m, true)))
}

fn info_for(r: &InstalledExtension, m: &ManifestInfo, enabled: bool) -> ExtensionInfo {
    ExtensionInfo {
        id: r.id.clone(),
        name: m.name.clone(),
        version: m.version.clone(),
        description: m.description.clone(),
        enabled,
        pinned: r.pinned,
        source: r.source.clone(),
        icon: icon_from_dir(Path::new(&r.path), m, 32),
        has_action: m.has_action,
        popup: m.popup.clone(),
        action_title: m.action_title.clone(),
        options_page: m.options_page.clone(),
    }
}

/// `(id, name, enabled)` for every extension in the WebView2 profile.
async fn profile_extensions(b: &SharedBrowser) -> Vec<(String, String, bool)> {
    let Some(ui) = b.ui.get().cloned() else { return Vec::new() };
    webview2::with_core_async(&ui, |_, core, tx: oneshot::Sender<Vec<(String, String, bool)>>| {
        let Some(p7) = webview2::profile(&core).and_then(|p| p.cast::<ICoreWebView2Profile7>().ok()) else {
            let _ = tx.send(Vec::new());
            return;
        };
        let tx = std::cell::RefCell::new(Some(tx));
        let handler = ProfileGetBrowserExtensionsCompletedHandler::create(Box::new(move |_, list| {
            let mut out = Vec::new();
            if let Some(list) = list {
                unsafe {
                    let mut n = 0u32;
                    let _ = list.Count(&mut n);
                    for i in 0..n {
                        let Ok(e) = list.GetValueAtIndex(i) else { continue };
                        let (mut id, mut name, mut on) = (PWSTR::null(), PWSTR::null(), BOOL::default());
                        let _ = e.Id(&mut id);
                        let _ = e.Name(&mut name);
                        let _ = e.IsEnabled(&mut on);
                        out.push((take_string(id), take_string(name), on.as_bool()));
                    }
                }
            }
            if let Some(tx) = tx.borrow_mut().take() {
                let _ = tx.send(out);
            }
            Ok(())
        }));
        unsafe {
            let _ = p7.GetBrowserExtensions(&handler);
        }
    })
    .await
    .unwrap_or_default()
}

pub async fn list(b: &SharedBrowser) -> Result<Vec<ExtensionInfo>, String> {
    let records = b.db.run(|db| registry::list(db.conn())).await?;
    let live = profile_extensions(b).await;
    let mut out = Vec::new();
    for r in &records {
        let Some((_, _, enabled)) = live.iter().find(|(id, _, _)| id == &r.id) else { continue };
        let Ok(m) = manifest::read_dir(Path::new(&r.path)) else { continue };
        out.push(info_for(r, &m, *enabled));
    }
    Ok(out)
}

/// Runs `f` with one extension of the profile (on the UI thread).
async fn with_extension<T: Send + 'static>(
    b: &SharedBrowser,
    id: String,
    f: impl FnOnce(ICoreWebView2BrowserExtension, oneshot::Sender<T>) + Send + 'static,
) -> Option<T> {
    let ui = b.ui.get().cloned()?;
    webview2::with_core_async(&ui, move |_, core, tx: oneshot::Sender<T>| {
        let Some(p7) = webview2::profile(&core).and_then(|p| p.cast::<ICoreWebView2Profile7>().ok()) else { return };
        let slot = std::cell::RefCell::new(Some((f, tx)));
        let handler = ProfileGetBrowserExtensionsCompletedHandler::create(Box::new(move |_, list| {
            let Some(list) = list else { return Ok(()) };
            unsafe {
                let mut n = 0u32;
                let _ = list.Count(&mut n);
                for i in 0..n {
                    let Ok(e) = list.GetValueAtIndex(i) else { continue };
                    let mut p = PWSTR::null();
                    let _ = e.Id(&mut p);
                    if take_string(p) == id {
                        if let Some((f, tx)) = slot.borrow_mut().take() {
                            f(e, tx);
                        }
                        break;
                    }
                }
            }
            Ok(())
        }));
        unsafe {
            let _ = p7.GetBrowserExtensions(&handler);
        }
    })
    .await
}

pub async fn remove(b: &SharedBrowser, id: String) -> Result<(), String> {
    let _ = with_extension(b, id.clone(), |e, tx: oneshot::Sender<()>| unsafe {
        let tx = std::cell::RefCell::new(Some(tx));
        let _ = e.Remove(&BrowserExtensionRemoveCompletedHandler::create(Box::new(move |_| {
            if let Some(tx) = tx.borrow_mut().take() {
                let _ = tx.send(());
            }
            Ok(())
        })));
    })
    .await;
    let i = id.clone();
    let record = b.db.run(move |db| registry::get(db.conn(), &i)).await?;
    if let Some(r) = record {
        let path = PathBuf::from(&r.path);
        // Only delete folders Limbo owns (never an unpacked developer folder).
        if path.starts_with(&b.paths.extensions)
            && let Some(id_dir) = path.parent()
        {
            let _ = std::fs::remove_dir_all(id_dir);
        }
        let i = id.clone();
        b.db.run(move |db| registry::remove(db.conn(), &i)).await?;
    }
    b.emit("ext:changed", ());
    Ok(())
}

pub async fn set_enabled(b: &SharedBrowser, id: String, on: bool) -> Result<(), String> {
    with_extension(b, id, move |e, tx: oneshot::Sender<()>| unsafe {
        let tx = std::cell::RefCell::new(Some(tx));
        let _ = e.Enable(
            on,
            &BrowserExtensionEnableCompletedHandler::create(Box::new(move |_| {
                if let Some(tx) = tx.borrow_mut().take() {
                    let _ = tx.send(());
                }
                Ok(())
            })),
        );
    })
    .await;
    b.emit("ext:changed", ());
    Ok(())
}

pub async fn set_pinned(b: &SharedBrowser, id: String, pinned: bool) -> Result<(), String> {
    b.db.run(move |db| registry::set_pinned(db.conn(), &id, pinned)).await?;
    b.emit("ext:changed", ());
    Ok(())
}

// --- popups ------------------------------------------------------------------

const POPUP_LABEL: &str = "ext-popup";
/// Reports the popup's content size so the webview can hug it.
const POPUP_SIZE_JS: &str = r#"(() => {
  if (window.top !== window) return;
  const send = () => {
    const d = document.documentElement, body = document.body;
    const w = Math.max(d.scrollWidth, body ? body.scrollWidth : 0);
    const h = Math.max(d.scrollHeight, body ? body.scrollHeight : 0);
    try { chrome.webview.postMessage(JSON.stringify({ limboPopup: 1, w, h })); } catch (_) {}
  };
  addEventListener('DOMContentLoaded', () => { send(); new ResizeObserver(send).observe(document.documentElement); });
  addEventListener('load', send);
})();"#;

#[derive(Debug, Clone, Copy, Deserialize)]
pub struct Anchor {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

pub fn close_popup(b: &SharedBrowser) {
    if let Some(wv) = b.app.get_webview(POPUP_LABEL) {
        let _ = wv.close();
        b.emit("ext:popup-closed", ());
    }
}

/// Opens an extension's toolbar popup under its icon (CSS px anchor).
pub async fn open_popup(b: &SharedBrowser, id: String, popup: String, anchor: Anchor) -> Result<(), String> {
    close_popup(b);
    let url: url::Url = format!("chrome-extension://{id}/{}", popup.trim_start_matches('/'))
        .parse()
        .map_err(|e: url::ParseError| e.to_string())?;
    let settings = b.settings();
    let mut builder = WebviewBuilder::new(POPUP_LABEL, WebviewUrl::External(url))
        .data_directory(b.paths.profile.clone())
        .additional_browser_args(&settings.browser_args())
        .browser_extensions_enabled(true)
        .initialization_script(POPUP_SIZE_JS)
        .focused(true);
    if let Some(env) = b.env.get() {
        builder = builder.with_environment(env.0.clone());
    }
    let (w, h) = (320.0, 120.0);
    let x = (anchor.x + anchor.width - w).max(8.0);
    let y = anchor.y + anchor.height + 6.0;
    let wv =
        b.window.add_child(builder, LogicalPosition::new(x, y), LogicalSize::new(w, h)).map_err(|e| e.to_string())?;
    let right = anchor.x + anchor.width;
    let weak = std::sync::Arc::downgrade(b);
    webview2::with_core(&wv, move |controller, core| unsafe {
        // Size to content (25x25 .. 800x600), keeping the right edge under the icon.
        let w2 = weak.clone();
        let mut token = 0i64;
        let _ = core.add_WebMessageReceived(
            &webview2_com::WebMessageReceivedEventHandler::create(Box::new(move |_, args| {
                let (Some(b), Some(args)) = (w2.upgrade(), args) else { return Ok(()) };
                let mut raw = PWSTR::null();
                if args.TryGetWebMessageAsString(&mut raw).is_err() {
                    return Ok(());
                }
                let v: serde_json::Value = serde_json::from_str(&take_string(raw)).unwrap_or_default();
                if v.get("limboPopup").is_none() {
                    return Ok(());
                }
                let w = v.get("w").and_then(|x| x.as_f64()).unwrap_or(320.0).clamp(25.0, 800.0);
                let h = v.get("h").and_then(|x| x.as_f64()).unwrap_or(120.0).clamp(25.0, 600.0);
                if let Some(wv) = b.app.get_webview(POPUP_LABEL) {
                    let x = (right - w).max(8.0);
                    let _ = wv.set_bounds(tauri::Rect {
                        position: LogicalPosition::new(x, y).into(),
                        size: LogicalSize::new(w, h).into(),
                    });
                }
                Ok(())
            })),
            &mut token,
        );
        // Clicking anywhere else closes it.
        let w3 = weak.clone();
        let _ = controller.add_LostFocus(
            &FocusChangedEventHandler::create(Box::new(move |_, _| {
                if let Some(b) = w3.upgrade() {
                    tauri::async_runtime::spawn(async move { close_popup(&b) });
                }
                Ok(())
            })),
            &mut token,
        );
    });
    let _ = wv.set_focus();
    Ok(())
}

pub async fn open_options(b: &SharedBrowser, id: String, page: String) -> Result<(), String> {
    let url = format!("chrome-extension://{id}/{}", page.trim_start_matches('/'));
    crate::tabs::create(b, Some(url), Default::default()).await.map(|_| ())
}

// --- updates -------------------------------------------------------------------

/// Checks for updates shortly after startup and then daily.
pub fn start_updater(b: &SharedBrowser) {
    let b = b.clone();
    std::thread::Builder::new()
        .name("limbo-ext-updates".into())
        .spawn(move || {
            std::thread::sleep(Duration::from_secs(90));
            loop {
                if let Err(e) = check_updates(&b) {
                    log::info!("extension update check: {e}");
                }
                std::thread::sleep(Duration::from_secs(24 * 3600));
            }
        })
        .expect("spawn updater");
}

fn check_updates(b: &SharedBrowser) -> Result<usize, String> {
    let records = b.db.call_blocking(|db| registry::list(db.conn()).unwrap_or_default()).unwrap_or_default();
    let mut updated = 0;
    for store in [Store::ChromeWebStore, Store::EdgeAddons] {
        let items: Vec<(String, String)> =
            records.iter().filter(|r| r.source == store.as_str()).map(|r| (r.id.clone(), r.version.clone())).collect();
        if items.is_empty() {
            continue;
        }
        let url = update::check_url(store.update_base(), &prodversion(), &items);
        let xml = crate::http::get_string(&b.http, &url, Duration::from_secs(30))?;
        for u in update::parse_response(&xml) {
            let Some(current) = items.iter().find(|(id, _)| *id == u.id) else { continue };
            if update::compare_versions(&u.version, &current.1) != std::cmp::Ordering::Greater {
                continue;
            }
            let bytes = crate::http::get_bytes(&b.http, &u.codebase, 256 * 1024 * 1024)?;
            if let Some(expected) = &u.sha256 {
                use sha2::Digest;
                let got: String = sha2::Sha256::digest(&bytes).iter().map(|x| format!("{x:02x}")).collect();
                if !got.eq_ignore_ascii_case(expected) {
                    log::warn!("update for {} failed its hash check", u.id);
                    continue;
                }
            }
            let v = crx::verify(&bytes, Some(&u.id)).map_err(|e| e.to_string())?;
            let m = manifest::read_zip(v.archive).map_err(|e| e.to_string())?;
            let staged = Staged {
                archive: v.archive.to_vec(),
                id: Some(v.id.clone()),
                public_key: Some(v.public_key.clone()),
                manifest: m,
                source: store.as_str().to_string(),
            };
            let token = stage(staged).token;
            let b2 = b.clone();
            match tauri::async_runtime::block_on(async move { confirm(&b2, token).await }) {
                Ok(_) => updated += 1,
                Err(e) => log::warn!("updating {} failed: {e}", u.id),
            }
        }
    }
    Ok(updated)
}
