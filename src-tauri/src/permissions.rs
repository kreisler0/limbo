//! Site permission prompts (camera, microphone, location, notifications, ...),
//! anchored to the omnibox and remembered per origin when asked to.

use std::collections::HashMap;
use std::sync::Weak;

use parking_lot::Mutex;
use serde::Serialize;
use webview2_com::Microsoft::Web::WebView2::Win32::*;
use webview2_com::PermissionRequestedEventHandler;
use windows_core::{BOOL, Interface, PWSTR};

use crate::pending::ThreadBound;
use crate::state::{Browser, SharedBrowser, TabId};
use crate::webview2::take_string;

/// Remembered decisions, loaded once (small) so prompts never wait on disk.
static REMEMBERED: Mutex<Option<HashMap<(String, String), bool>>> = Mutex::new(None);

pub fn init(rows: Vec<limbo_core::sites::SitePermission>) {
    let map = rows.into_iter().map(|r| ((r.origin, r.kind), r.allow)).collect();
    *REMEMBERED.lock() = Some(map);
}

fn kind_name(k: COREWEBVIEW2_PERMISSION_KIND) -> &'static str {
    match k {
        COREWEBVIEW2_PERMISSION_KIND_MICROPHONE => "microphone",
        COREWEBVIEW2_PERMISSION_KIND_CAMERA => "camera",
        COREWEBVIEW2_PERMISSION_KIND_GEOLOCATION => "location",
        COREWEBVIEW2_PERMISSION_KIND_NOTIFICATIONS => "notifications",
        COREWEBVIEW2_PERMISSION_KIND_OTHER_SENSORS => "sensors",
        COREWEBVIEW2_PERMISSION_KIND_CLIPBOARD_READ => "clipboard",
        COREWEBVIEW2_PERMISSION_KIND_MULTIPLE_AUTOMATIC_DOWNLOADS => "multipleDownloads",
        COREWEBVIEW2_PERMISSION_KIND_FILE_READ_WRITE => "files",
        COREWEBVIEW2_PERMISSION_KIND_AUTOPLAY => "autoplay",
        COREWEBVIEW2_PERMISSION_KIND_LOCAL_FONTS => "fonts",
        COREWEBVIEW2_PERMISSION_KIND_MIDI_SYSTEM_EXCLUSIVE_MESSAGES => "midi",
        COREWEBVIEW2_PERMISSION_KIND_WINDOW_MANAGEMENT => "windowManagement",
        _ => "other",
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PermissionPrompt {
    pub request_id: u64,
    pub tab_id: TabId,
    pub origin: String,
    pub kind: &'static str,
}

pub fn wire(weak: &Weak<Browser>, id: TabId, core: &ICoreWebView2) -> windows_core::Result<()> {
    let w = weak.clone();
    let mut token = 0i64;
    unsafe {
        core.add_PermissionRequested(
            &PermissionRequestedEventHandler::create(Box::new(move |_, args| {
                let (Some(b), Some(args)) = (w.upgrade(), args) else { return Ok(()) };
                let mut uri = PWSTR::null();
                args.Uri(&mut uri)?;
                let uri = take_string(uri);
                let mut kind = COREWEBVIEW2_PERMISSION_KIND_UNKNOWN_PERMISSION;
                args.PermissionKind(&mut kind)?;
                let name = kind_name(kind);
                let Some(origin) = limbo_core::omnibox::origin_of(&uri) else {
                    args.SetState(COREWEBVIEW2_PERMISSION_STATE_DENY)?;
                    return Ok(());
                };
                // Autoplay and fonts need no prompt.
                if matches!(name, "autoplay") {
                    return Ok(());
                }
                if let Ok(a3) = args.cast::<ICoreWebView2PermissionRequestedEventArgs3>() {
                    // Limbo remembers decisions itself.
                    let _ = a3.SetSavesInProfile(false);
                }
                let private = b.inner.lock().tab(id).map(|t| t.info.private).unwrap_or(false);
                let remembered = if private {
                    None
                } else {
                    REMEMBERED.lock().as_ref().and_then(|m| m.get(&(origin.clone(), name.to_string())).copied())
                };
                if let Some(allow) = remembered {
                    args.SetState(if allow {
                        COREWEBVIEW2_PERMISSION_STATE_ALLOW
                    } else {
                        COREWEBVIEW2_PERMISSION_STATE_DENY
                    })?;
                    if allow {
                        mark_capturing(&b, id, name);
                    }
                    return Ok(());
                }
                let deferral = args.GetDeferral()?;
                let held = ThreadBound::new((args.clone(), deferral));
                let b2 = b.clone();
                let origin2 = origin.clone();
                let request_id = b.pending.add(Box::new(move |answer| {
                    let allow = answer.get("allow").and_then(|v| v.as_bool()).unwrap_or(false);
                    let remember = answer.get("remember").and_then(|v| v.as_bool()).unwrap_or(false) && !private;
                    let b3 = b2.clone();
                    let origin3 = origin2.clone();
                    let _ = b2.app.run_on_main_thread(move || {
                        let Some((args, deferral)) = held.into_inner() else { return };
                        let _ = args.SetState(if allow {
                            COREWEBVIEW2_PERMISSION_STATE_ALLOW
                        } else {
                            COREWEBVIEW2_PERMISSION_STATE_DENY
                        });
                        let _ = deferral.Complete();
                        if allow {
                            mark_capturing(&b3, id, name);
                        }
                    });
                    if remember {
                        remember_decision(&b2, origin3, name, allow);
                    }
                }));
                b.emit("perm:request", PermissionPrompt { request_id, tab_id: id, origin, kind: name });
                let _ = BOOL::default();
                Ok(())
            })),
            &mut token,
        )?;
    }
    Ok(())
}

/// Tabs using the camera or microphone are exempt from suspension.
fn mark_capturing(b: &SharedBrowser, id: TabId, kind: &str) {
    if !matches!(kind, "camera" | "microphone") {
        return;
    }
    let flags = {
        let mut inner = b.inner.lock();
        let Some(t) = inner.tab_mut(id) else { return };
        t.flags.capturing = true;
        t.flags
    };
    let now = b.now_ms();
    let actions = b.inner.lock().lifecycle.set_flags(id, flags, now);
    crate::tabs::apply_actions(b, actions);
}

pub fn remember_decision(b: &SharedBrowser, origin: String, kind: &str, allow: bool) {
    REMEMBERED.lock().get_or_insert_with(HashMap::new).insert((origin.clone(), kind.to_string()), allow);
    let kind = kind.to_string();
    b.db.spawn(move |db| {
        let _ = limbo_core::sites::set_permission(db.conn(), &origin, &kind, allow, limbo_core::db::now_us());
    });
}

pub fn forget(b: &SharedBrowser, origin: String, kind: String) {
    if let Some(m) = REMEMBERED.lock().as_mut() {
        m.remove(&(origin.clone(), kind.clone()));
    }
    b.db.spawn(move |db| {
        let _ = limbo_core::sites::revoke_permission(db.conn(), &origin, &kind);
    });
}
