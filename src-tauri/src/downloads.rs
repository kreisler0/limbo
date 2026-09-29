//! Downloads: WebView2's flyout is suppressed and Limbo shows its own popover
//! (progress rings, pause/resume/cancel, show in folder, danger warnings).

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Weak;
use std::sync::atomic::{AtomicI64, Ordering};
use std::time::Instant;

use limbo_core::downloads::{self as store, DownloadRecord, DownloadState};
use parking_lot::Mutex;
use serde::Serialize;
use webview2_com::Microsoft::Web::WebView2::Win32::*;
use webview2_com::{BytesReceivedChangedEventHandler, DownloadStartingEventHandler, StateChangedEventHandler};
use windows_core::{BOOL, HSTRING, Interface, PWSTR};

use crate::pending::ThreadBound;
use crate::state::{Browser, SharedBrowser, TabId};
use crate::webview2::take_string;

static NEXT_ID: AtomicI64 = AtomicI64::new(0);
static OPS: Mutex<Option<HashMap<i64, ThreadBound<ICoreWebView2DownloadOperation>>>> = Mutex::new(None);
static LAST_EMIT: Mutex<Option<HashMap<i64, Instant>>> = Mutex::new(None);

/// Seeds ids after the highest stored one (called at startup).
pub fn init(max_id: i64) {
    NEXT_ID.store(max_id, Ordering::Relaxed);
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Progress {
    pub id: i64,
    pub tab_id: Option<TabId>,
    pub url: String,
    pub path: String,
    pub file_name: String,
    pub received: i64,
    pub total: Option<i64>,
    pub state: DownloadState,
    pub danger: Option<String>,
    pub can_resume: bool,
}

impl Progress {
    pub fn from_record(r: &DownloadRecord) -> Progress {
        Progress {
            id: r.id,
            tab_id: None,
            url: r.url.clone(),
            file_name: file_name(&r.path),
            path: r.path.clone(),
            received: r.received_bytes,
            total: r.total_bytes,
            state: r.state,
            danger: r.danger.clone(),
            can_resume: false,
        }
    }
}

fn file_name(path: &str) -> String {
    Path::new(path).file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| path.to_string())
}

unsafe fn read(op: &ICoreWebView2DownloadOperation, id: i64, tab: TabId) -> Progress {
    unsafe {
        let mut p = PWSTR::null();
        let _ = op.Uri(&mut p);
        let url = take_string(p);
        let mut p = PWSTR::null();
        let _ = op.ResultFilePath(&mut p);
        let path = take_string(p);
        let mut received = 0i64;
        let _ = op.BytesReceived(&mut received);
        let mut total = 0i64;
        let _ = op.TotalBytesToReceive(&mut total);
        let mut state = COREWEBVIEW2_DOWNLOAD_STATE_IN_PROGRESS;
        let _ = op.State(&mut state);
        let mut reason = COREWEBVIEW2_DOWNLOAD_INTERRUPT_REASON_NONE;
        let _ = op.InterruptReason(&mut reason);
        let mut can_resume = BOOL::default();
        let _ = op.CanResume(&mut can_resume);
        let (state, danger) = match state {
            COREWEBVIEW2_DOWNLOAD_STATE_COMPLETED => (DownloadState::Completed, None),
            COREWEBVIEW2_DOWNLOAD_STATE_INTERRUPTED => {
                let danger = match reason {
                    COREWEBVIEW2_DOWNLOAD_INTERRUPT_REASON_FILE_BLOCKED_BY_POLICY => Some("blocked"),
                    COREWEBVIEW2_DOWNLOAD_INTERRUPT_REASON_FILE_MALICIOUS => Some("malicious"),
                    COREWEBVIEW2_DOWNLOAD_INTERRUPT_REASON_FILE_SECURITY_CHECK_FAILED => Some("securityCheckFailed"),
                    _ => None,
                };
                let st = if reason == COREWEBVIEW2_DOWNLOAD_INTERRUPT_REASON_USER_CANCELED {
                    DownloadState::Cancelled
                } else if reason == COREWEBVIEW2_DOWNLOAD_INTERRUPT_REASON_USER_PAUSED {
                    DownloadState::Paused
                } else {
                    DownloadState::Interrupted
                };
                (st, danger.map(str::to_string))
            }
            _ => (DownloadState::InProgress, None),
        };
        Progress {
            id,
            tab_id: Some(tab),
            url,
            file_name: file_name(&path),
            path,
            received,
            total: (total > 0).then_some(total),
            state,
            danger,
            can_resume: can_resume.as_bool(),
        }
    }
}

fn emit(b: &SharedBrowser, p: Progress, force: bool) {
    if !force {
        let mut last = LAST_EMIT.lock();
        let map = last.get_or_insert_with(HashMap::new);
        let now = Instant::now();
        if map.get(&p.id).is_some_and(|t| now.duration_since(*t).as_millis() < 250) {
            return;
        }
        map.insert(p.id, now);
    }
    let rec = p.clone();
    b.db.spawn(move |db| {
        let _ = store::update(
            db.conn(),
            rec.id,
            rec.received,
            rec.total,
            rec.state,
            rec.danger.as_deref(),
            limbo_core::db::now_us(),
        );
        let _ = store::set_path(db.conn(), rec.id, &rec.path);
    });
    b.emit("download:progress", p);
}

pub fn wire(weak: &Weak<Browser>, tab: TabId, core: &ICoreWebView2) -> windows_core::Result<()> {
    let Ok(c4) = core.cast::<ICoreWebView2_4>() else { return Ok(()) };
    let w = weak.clone();
    let mut token = 0i64;
    unsafe {
        c4.add_DownloadStarting(
            &DownloadStartingEventHandler::create(Box::new(move |_, args| {
                let (Some(b), Some(args)) = (w.upgrade(), args) else { return Ok(()) };
                // Our popover replaces the default flyout.
                args.SetHandled(true)?;
                let op = args.DownloadOperation()?;
                let (folder, ask) = {
                    let s = b.settings();
                    (s.downloads_folder.clone(), s.ask_where_to_save)
                };
                let mut p = PWSTR::null();
                let _ = args.ResultFilePath(&mut p);
                let default_path = PathBuf::from(take_string(p));
                if let (Some(folder), Some(name)) = (folder, default_path.file_name()) {
                    let target = unique_path(&PathBuf::from(folder).join(name));
                    let _ = args.SetResultFilePath(&HSTRING::from(target.as_os_str()));
                }
                let id = NEXT_ID.fetch_add(1, Ordering::Relaxed) + 1;
                track(&b, id, tab, &op);
                if ask {
                    let deferral = args.GetDeferral()?;
                    let held = ThreadBound::new((args.clone(), deferral));
                    let b2 = b.clone();
                    let suggested =
                        default_path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
                    tauri::async_runtime::spawn(async move {
                        let chosen = crate::dialog::save_file(&b2, "Save as", &suggested, &[]).await;
                        let _ = b2.app.run_on_main_thread(move || {
                            let Some((args, deferral)) = held.into_inner() else { return };
                            match chosen {
                                Some(path) => {
                                    let _ = args.SetResultFilePath(&HSTRING::from(path.as_os_str()));
                                }
                                None => {
                                    let _ = args.SetCancel(true);
                                }
                            }
                            let _ = deferral.Complete();
                        });
                    });
                }
                Ok(())
            })),
            &mut token,
        )?;
    }
    Ok(())
}

fn unique_path(path: &Path) -> PathBuf {
    if !path.exists() {
        return path.to_path_buf();
    }
    let stem = path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
    let ext = path.extension().map(|e| format!(".{}", e.to_string_lossy())).unwrap_or_default();
    for n in 1..1000 {
        let candidate = path.with_file_name(format!("{stem} ({n}){ext}"));
        if !candidate.exists() {
            return candidate;
        }
    }
    path.to_path_buf()
}

unsafe fn track(b: &SharedBrowser, id: i64, tab: TabId, op: &ICoreWebView2DownloadOperation) {
    let first = unsafe { read(op, id, tab) };
    let rec = first.clone();
    let mut mime = PWSTR::null();
    let mime = unsafe { op.MimeType(&mut mime).ok().map(|_| take_string(mime)) };
    b.db.spawn(move |db| {
        let _ = store::insert(
            db.conn(),
            Some(rec.id),
            &rec.url,
            &rec.path,
            mime.as_deref(),
            rec.total,
            limbo_core::db::now_us(),
        );
    });
    OPS.lock().get_or_insert_with(HashMap::new).insert(id, ThreadBound::new(op.clone()));
    mark_downloading(b, tab, true);
    emit(b, first, true);

    let weak = std::sync::Arc::downgrade(b);
    let mut token = 0i64;
    unsafe {
        let w = weak.clone();
        let _ = op.add_BytesReceivedChanged(
            &BytesReceivedChangedEventHandler::create(Box::new(move |op, _| {
                if let (Some(b), Some(op)) = (w.upgrade(), op) {
                    emit(&b, read(&op, id, tab), false);
                }
                Ok(())
            })),
            &mut token,
        );
        let w = weak.clone();
        let _ = op.add_StateChanged(
            &StateChangedEventHandler::create(Box::new(move |op, _| {
                if let (Some(b), Some(op)) = (w.upgrade(), op) {
                    let p = read(&op, id, tab);
                    let done = matches!(
                        p.state,
                        DownloadState::Completed | DownloadState::Cancelled | DownloadState::Interrupted
                    );
                    emit(&b, p, true);
                    if done {
                        if let Some(map) = OPS.lock().as_mut() {
                            map.remove(&id);
                        }
                        mark_downloading(&b, tab, false);
                    }
                }
                Ok(())
            })),
            &mut token,
        );
    }
}

/// Tabs with an active download are never suspended or discarded.
fn mark_downloading(b: &SharedBrowser, tab: TabId, on: bool) {
    let flags = {
        let mut inner = b.inner.lock();
        let Some(t) = inner.tab_mut(tab) else { return };
        t.flags.downloading = on;
        t.flags
    };
    let now = b.now_ms();
    let actions = b.inner.lock().lifecycle.set_flags(tab, flags, now);
    crate::tabs::apply_actions(b, actions);
    b.lifecycle_signal.notify();
}

/// pause | resume | cancel on the UI thread.
pub fn control(b: &SharedBrowser, id: i64, what: &'static str) {
    let _ = b.app.run_on_main_thread(move || {
        let guard = OPS.lock();
        let Some(op) = guard.as_ref().and_then(|m| m.get(&id)).and_then(|t| t.get()) else { return };
        unsafe {
            let _ = match what {
                "pause" => op.Pause(),
                "resume" => op.Resume(),
                "cancel" => op.Cancel(),
                _ => Ok(()),
            };
        }
    });
}

pub async fn list(b: &SharedBrowser) -> Result<Vec<Progress>, String> {
    let records = b.db.run(|db| store::list(db.conn(), 100)).await?;
    Ok(records.iter().map(Progress::from_record).collect())
}

pub fn open_file(path: &str) {
    use windows::Win32::UI::Shell::ShellExecuteW;
    use windows::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;
    unsafe {
        ShellExecuteW(None, &HSTRING::from("open"), &HSTRING::from(path), None, None, SW_SHOWNORMAL);
    }
}

pub fn show_in_folder(path: &str) {
    let _ = std::process::Command::new("explorer.exe").arg(format!("/select,{path}")).spawn();
}
