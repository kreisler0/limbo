//! Runs a Firefox import type by type with progress events. Cookies go into
//! WebView2's cookie manager; Firefox's open tabs come back as unloaded tabs.

use std::path::PathBuf;

use limbo_core::import::firefox::{
    self as ff, DataType, HistoryCounts, PasswordCounts, addons::FirefoxAddon, cookies::ImportedCookie,
    cookies::SameSite, profiles::FirefoxProfile,
};
use serde::{Deserialize, Serialize};
use tokio::sync::oneshot;
use webview2_com::Microsoft::Web::WebView2::Win32::*;
use windows_core::{HSTRING, Interface};

use crate::state::SharedBrowser;
use crate::webview2;

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RunArgs {
    pub profile_path: String,
    pub types: Vec<DataType>,
    #[serde(default)]
    pub primary_password: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Summary {
    pub history: Option<HistoryCounts>,
    pub bookmarks: Option<usize>,
    pub favicons: Option<usize>,
    pub passwords: Option<PasswordCounts>,
    pub cookies: Option<usize>,
    pub form_history: Option<usize>,
    pub tabs: Option<usize>,
    pub addons: Vec<FirefoxAddon>,
    pub errors: Vec<TypeError>,
    pub not_imported: Vec<String>,
    /// Passwords need the Firefox primary password (ask, then import passwords again).
    pub needs_primary_password: bool,
    pub firefox_running: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TypeError {
    pub data_type: DataType,
    pub message: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct ProgressEvent {
    data_type: DataType,
    status: &'static str,
    done: usize,
    total: usize,
}

pub fn detect() -> Vec<FirefoxProfile> {
    ff::profiles::default_root().and_then(|r| ff::profiles::discover(&r).ok()).unwrap_or_default()
}

pub async fn choose_folder(b: &SharedBrowser) -> Option<FirefoxProfile> {
    let dir = crate::dialog::pick_folder(b, "Choose a Firefox profile folder").await?;
    Some(ff::profiles::from_dir(&dir))
}

fn progress(b: &SharedBrowser, t: DataType, status: &'static str, done: usize, total: usize) {
    b.emit("import:progress", ProgressEvent { data_type: t, status, done, total });
}

pub async fn run(b: &SharedBrowser, args: RunArgs) -> Summary {
    let profile = PathBuf::from(&args.profile_path);
    let mut summary = Summary {
        not_imported: ff::NOT_IMPORTED.iter().map(|s| s.to_string()).collect(),
        firefox_running: ff::profiles::is_running(&profile),
        ..Default::default()
    };
    for t in DataType::ALL {
        if !args.types.contains(&t) {
            continue;
        }
        progress(b, t, "running", 0, 0);
        let p = profile.clone();
        let result: Result<(), String> = match t {
            DataType::History => {
                let b2 = b.clone();
                b.db.run(move |db| {
                    ff::import_history(db.conn_mut(), &p, |d, total| {
                        progress(&b2, DataType::History, "running", d, total)
                    })
                })
                .await
                .map(|c| summary.history = Some(c))
            }
            DataType::Bookmarks => {
                b.db.run(move |db| ff::import_bookmarks(db.conn_mut(), &p)).await.map(|n| summary.bookmarks = Some(n))
            }
            DataType::Favicons => {
                b.db.run(move |db| ff::import_favicons(db.conn_mut(), &p)).await.map(|n| summary.favicons = Some(n))
            }
            DataType::Passwords => {
                let protector = b.protector.clone();
                let pw = args.primary_password.clone().unwrap_or_default();
                match b.db.call(move |db| ff::import_passwords(db.conn_mut(), &*protector, &p, &pw)).await {
                    Ok(Ok(c)) => {
                        summary.passwords = Some(c);
                        Ok(())
                    }
                    Ok(Err(limbo_core::Error::WrongPrimaryPassword)) => {
                        summary.needs_primary_password = true;
                        Ok(())
                    }
                    Ok(Err(e)) => Err(e.to_string()),
                    Err(e) => Err(e),
                }
            }
            DataType::Cookies => match tauri::async_runtime::spawn_blocking(move || ff::read_cookies(&p)).await {
                Ok(Ok(cookies)) => {
                    let n = write_cookies(b, cookies).await;
                    summary.cookies = Some(n);
                    Ok(())
                }
                Ok(Err(e)) => Err(e.to_string()),
                Err(e) => Err(e.to_string()),
            },
            DataType::FormHistory => {
                b.db.run(move |db| ff::import_form_history(db.conn_mut(), &p))
                    .await
                    .map(|n| summary.form_history = Some(n))
            }
            DataType::Tabs => match ff::read_tabs(&profile) {
                Ok(tabs) => {
                    for t in &tabs {
                        let st = limbo_core::sessions::SessionTab {
                            url: t.url.clone(),
                            title: t.title.clone(),
                            pinned: t.pinned,
                            ..Default::default()
                        };
                        crate::session::add_restored(b, &st, false);
                    }
                    summary.tabs = Some(tabs.len());
                    crate::session::schedule_save(b);
                    Ok(())
                }
                Err(e) => Err(e.to_string()),
            },
            DataType::Addons => ff::read_addons(&profile).map(|a| summary.addons = a).map_err(|e| e.to_string()),
        };
        match result {
            Ok(()) => progress(b, t, "done", 0, 0),
            Err(message) => {
                progress(b, t, "failed", 0, 0);
                summary.errors.push(TypeError { data_type: t, message });
            }
        }
    }
    b.emit("import:finished", summary.clone());
    summary
}

/// Writes cookies through the UI webview's cookie manager (same profile as
/// every tab), in chunks so the UI thread stays responsive.
async fn write_cookies(b: &SharedBrowser, cookies: Vec<ImportedCookie>) -> usize {
    let Some(ui) = b.ui.get().cloned() else { return 0 };
    let total = cookies.len();
    let mut written = 0;
    for chunk in cookies.chunks(200) {
        let chunk = chunk.to_vec();
        let n = webview2::with_core_async(&ui, move |_, core, tx: oneshot::Sender<usize>| {
            let mut n = 0;
            if let Ok(manager) = core.cast::<ICoreWebView2_2>().and_then(|c| unsafe { c.CookieManager() }) {
                for c in &chunk {
                    unsafe {
                        let Ok(cookie) = manager.CreateCookie(
                            &HSTRING::from(c.name.as_str()),
                            &HSTRING::from(c.value.as_str()),
                            &HSTRING::from(c.host.as_str()),
                            &HSTRING::from(c.path.as_str()),
                        ) else {
                            continue;
                        };
                        let _ = cookie.SetExpires(c.expires);
                        let _ = cookie.SetIsHttpOnly(c.http_only);
                        let _ = cookie.SetIsSecure(c.secure || c.same_site == SameSite::None);
                        let _ = cookie.SetSameSite(match c.same_site {
                            SameSite::None => COREWEBVIEW2_COOKIE_SAME_SITE_KIND_NONE,
                            SameSite::Lax => COREWEBVIEW2_COOKIE_SAME_SITE_KIND_LAX,
                            SameSite::Strict => COREWEBVIEW2_COOKIE_SAME_SITE_KIND_STRICT,
                        });
                        if manager.AddOrUpdateCookie(&cookie).is_ok() {
                            n += 1;
                        }
                    }
                }
            }
            let _ = tx.send(n);
        })
        .await
        .unwrap_or(0);
        written += n;
        progress(b, DataType::Cookies, "running", written, total);
    }
    written
}
