//! Live memory readout and the system memory-pressure monitor (plan 6.2, 6.3).
//!
//! Metric: private working set per process (Task Manager's "Memory" column),
//! read for every process in one `NtQuerySystemInformation` call. The engine's
//! processes come from `GetProcessExtendedInfos`, which also maps renderer
//! processes to frames and so to tabs and extensions.

use std::collections::HashMap;
use std::sync::mpsc;
use std::time::Duration;

use limbo_core::memory::{self, MemorySample, PressureLevel, PressureMonitor, ProcessKind, ProcessSample};
use webview2_com::GetProcessExtendedInfosCompletedHandler;
use webview2_com::Microsoft::Web::WebView2::Win32::*;
use windows::Win32::Foundation::{CloseHandle, HANDLE, WAIT_OBJECT_0};
use windows::Win32::System::Memory::{CreateMemoryResourceNotification, LowMemoryResourceNotification};
use windows::Win32::System::SystemInformation::{GlobalMemoryStatusEx, MEMORYSTATUSEX};
use windows::Win32::System::Threading::{GetCurrentProcessId, WaitForSingleObject};
use windows_core::Interface;

use crate::state::SharedBrowser;

/// `(total, available)` physical memory in bytes.
pub fn system_memory() -> (u64, u64) {
    let mut s = MEMORYSTATUSEX { dwLength: std::mem::size_of::<MEMORYSTATUSEX>() as u32, ..Default::default() };
    unsafe {
        if GlobalMemoryStatusEx(&mut s).is_ok() {
            return (s.ullTotalPhys, s.ullAvailPhys);
        }
    }
    (0, 0)
}

/// Private working set of every process, by PID.
fn private_working_sets(buf: &mut Vec<u8>) -> HashMap<u32, u64> {
    use windows::Wdk::System::SystemInformation::{NtQuerySystemInformation, SYSTEM_INFORMATION_CLASS};
    const SYSTEM_PROCESS_INFORMATION: SYSTEM_INFORMATION_CLASS = SYSTEM_INFORMATION_CLASS(5);
    const STATUS_INFO_LENGTH_MISMATCH: i32 = 0xC0000004u32 as i32;
    if buf.is_empty() {
        buf.resize(512 * 1024, 0);
    }
    loop {
        let mut needed = 0u32;
        let status = unsafe {
            NtQuerySystemInformation(
                SYSTEM_PROCESS_INFORMATION,
                buf.as_mut_ptr() as *mut _,
                buf.len() as u32,
                &mut needed,
            )
        };
        if status.0 == STATUS_INFO_LENGTH_MISMATCH {
            let grow = (needed as usize).max(buf.len() * 2) + 64 * 1024;
            if grow > 16 * 1024 * 1024 {
                return HashMap::new();
            }
            buf.resize(grow, 0);
            continue;
        }
        if status.0 < 0 {
            return HashMap::new();
        }
        break;
    }
    // SYSTEM_PROCESS_INFORMATION (x64): NextEntryOffset u32 @0,
    // WorkingSetPrivateSize i64 @8, UniqueProcessId HANDLE @0x50.
    let mut out = HashMap::new();
    let mut off = 0usize;
    loop {
        if off + 0x58 > buf.len() {
            break;
        }
        let next = u32::from_le_bytes(buf[off..off + 4].try_into().unwrap()) as usize;
        let ws_private = i64::from_le_bytes(buf[off + 8..off + 16].try_into().unwrap());
        let pid = usize::from_le_bytes(buf[off + 0x50..off + 0x58].try_into().unwrap()) as u32;
        out.insert(pid, ws_private.max(0) as u64);
        if next == 0 {
            break;
        }
        off += next;
    }
    out
}

/// One engine process as reported by WebView2.
struct EngineProcess {
    pid: u32,
    kind: ProcessKind,
    /// Main-frame ids of frames hosted by this process (top-level ancestor ids).
    frame_roots: Vec<u32>,
    extensions: Vec<String>,
}

fn kind_of(k: COREWEBVIEW2_PROCESS_KIND) -> ProcessKind {
    match k {
        COREWEBVIEW2_PROCESS_KIND_BROWSER => ProcessKind::Browser,
        COREWEBVIEW2_PROCESS_KIND_GPU => ProcessKind::Gpu,
        COREWEBVIEW2_PROCESS_KIND_RENDERER => ProcessKind::Renderer,
        COREWEBVIEW2_PROCESS_KIND_UTILITY => ProcessKind::Utility,
        _ => ProcessKind::Other,
    }
}

unsafe fn frame_root_id(info: &ICoreWebView2FrameInfo) -> Option<u32> {
    let mut cur = info.cast::<ICoreWebView2FrameInfo2>().ok()?;
    for _ in 0..32 {
        match unsafe { cur.ParentFrameInfo() } {
            Ok(parent) => match parent.cast::<ICoreWebView2FrameInfo2>() {
                Ok(p) => cur = p,
                Err(_) => break,
            },
            Err(_) => break,
        }
    }
    let mut id = 0u32;
    unsafe { cur.FrameId(&mut id).ok()? };
    Some(id)
}

fn extension_of(url: &str) -> Option<String> {
    let rest = url.strip_prefix("chrome-extension://")?;
    let id = rest.split('/').next()?;
    limbo_core::extensions::is_valid_id(id).then(|| id.to_string())
}

/// Asks WebView2 (on the UI thread) for its processes and their frames.
fn engine_processes(b: &SharedBrowser) -> Option<Vec<EngineProcess>> {
    b.env.get()?;
    let (tx, rx) = mpsc::channel::<Vec<EngineProcess>>();
    let b2 = b.clone();
    b.app
        .run_on_main_thread(move || {
            // COM calls stay on the UI thread, where the environment lives.
            let Some(env13) = b2.env.get().and_then(|e| e.0.cast::<ICoreWebView2Environment13>().ok()) else {
                let _ = tx.send(Vec::new());
                return;
            };
            let handler = GetProcessExtendedInfosCompletedHandler::create(Box::new(move |hr, collection| {
                let mut out = Vec::new();
                if let (Ok(()), Some(c)) = (hr, collection) {
                    unsafe {
                        let mut count = 0u32;
                        let _ = c.Count(&mut count);
                        for i in 0..count {
                            let Ok(ext) = c.GetValueAtIndex(i) else { continue };
                            let Ok(pi) = ext.ProcessInfo() else { continue };
                            let mut pid = 0i32;
                            let mut kind = COREWEBVIEW2_PROCESS_KIND_BROWSER;
                            let _ = pi.ProcessId(&mut pid);
                            let _ = pi.Kind(&mut kind);
                            let mut roots = Vec::new();
                            let mut exts = Vec::new();
                            if let Ok(frames) = ext.AssociatedFrameInfos()
                                && let Ok(it) = frames.GetIterator()
                            {
                                let mut has = windows_core::BOOL::default();
                                while it.HasCurrent(&mut has).is_ok() && has.as_bool() {
                                    if let Ok(f) = it.GetCurrent() {
                                        if let Some(r) = frame_root_id(&f)
                                            && !roots.contains(&r)
                                        {
                                            roots.push(r);
                                        }
                                        let mut src = windows_core::PWSTR::null();
                                        if f.Source(&mut src).is_ok()
                                            && let Some(e) = extension_of(&crate::webview2::take_string(src))
                                            && !exts.contains(&e)
                                        {
                                            exts.push(e);
                                        }
                                    }
                                    let mut moved = windows_core::BOOL::default();
                                    if it.MoveNext(&mut moved).is_err() || !moved.as_bool() {
                                        break;
                                    }
                                }
                            }
                            out.push(EngineProcess {
                                pid: pid as u32,
                                kind: kind_of(kind),
                                frame_roots: roots,
                                extensions: exts,
                            });
                        }
                    }
                }
                let _ = tx.send(out);
                Ok(())
            }));
            unsafe {
                let _ = env13.GetProcessExtendedInfos(&handler);
            }
        })
        .ok()?;
    rx.recv_timeout(Duration::from_secs(3)).ok()
}

/// Takes one sample (worker thread).
pub fn sample(b: &SharedBrowser, buf: &mut Vec<u8>) -> Option<MemorySample> {
    let engine = engine_processes(b)?;
    let ws = private_working_sets(buf);
    let (frame_to_tab, ui_frame, statuses, max_awake, awake) = {
        let inner = b.inner.lock();
        let map: HashMap<u32, u64> =
            inner.tabs.iter().filter_map(|t| t.main_frame_id.map(|f| (f, t.info.id))).collect();
        (
            map,
            crate::UI_FRAME_ID.load(std::sync::atomic::Ordering::Relaxed),
            inner.lifecycle.statuses(),
            inner.lifecycle.policy().max_awake,
            inner.lifecycle.awake_count(),
        )
    };
    let host_pid = unsafe { GetCurrentProcessId() };
    let mut procs = vec![ProcessSample {
        pid: host_pid,
        kind: ProcessKind::Host,
        bytes: ws.get(&host_pid).copied().unwrap_or(0),
        tabs: vec![],
        extensions: vec![],
        is_ui: false,
    }];
    for p in engine {
        let tabs: Vec<u64> = p.frame_roots.iter().filter_map(|f| frame_to_tab.get(f).copied()).collect();
        let is_ui = ui_frame != 0 && p.frame_roots.contains(&ui_frame) && tabs.is_empty();
        procs.push(ProcessSample {
            pid: p.pid,
            kind: p.kind,
            bytes: ws.get(&p.pid).copied().unwrap_or(0),
            tabs,
            extensions: p.extensions,
            is_ui,
        });
    }
    let (total, rows) = memory::attribute(&procs, &statuses);
    let (sys_total, sys_avail) = system_memory();
    Some(MemorySample {
        total_bytes: total,
        system_avail_bytes: sys_avail,
        system_total_bytes: sys_total,
        awake,
        max_awake,
        rows,
    })
}

/// Starts the sampler (only while someone is looking) and the pressure monitor.
pub fn start(b: &SharedBrowser) {
    let sb = b.clone();
    std::thread::Builder::new()
        .name("limbo-memory-sampler".into())
        .spawn(move || {
            let mut buf = Vec::new();
            loop {
                let (mode, minimized, show_pill, ready) = {
                    let inner = sb.inner.lock();
                    (inner.sampling, inner.minimized, inner.settings.show_memory_in_toolbar, inner.ui_ready)
                };
                let mode = if minimized || !ready || (!show_pill && mode == memory::SamplingMode::Pill) {
                    memory::SamplingMode::Off
                } else {
                    mode
                };
                match mode.interval_ms() {
                    None => {
                        buf = Vec::new(); // give the buffer back while idle
                        sb.sampler_signal.wait(None);
                        continue;
                    }
                    Some(ms) => {
                        if let Some(s) = sample(&sb, &mut buf) {
                            sb.emit("memory:sample", s);
                        }
                        sb.sampler_signal.wait(Some(Duration::from_millis(ms)));
                    }
                }
            }
        })
        .expect("spawn sampler");

    let pb = b.clone();
    std::thread::Builder::new()
        .name("limbo-memory-pressure".into())
        .spawn(move || {
            let low: Option<HANDLE> = unsafe { CreateMemoryResourceNotification(LowMemoryResourceNotification).ok() };
            let mut monitor = PressureMonitor::default();
            let mut last_level = PressureLevel::Normal;
            loop {
                let os_low = match low {
                    Some(h) => (unsafe { WaitForSingleObject(h, 5000) }) == WAIT_OBJECT_0,
                    None => {
                        std::thread::sleep(Duration::from_secs(5));
                        false
                    }
                };
                let (total, avail) = system_memory();
                let level = PressureMonitor::level(avail, total);
                if level != last_level {
                    last_level = level;
                    pb.emit("memory:pressure", level);
                }
                if monitor.update(avail, total, os_low) {
                    let now = pb.now_ms();
                    let action = pb.inner.lock().lifecycle.relieve_pressure(now);
                    if let Some(a) = action {
                        log::info!("memory pressure ({avail} of {total} bytes free): {a:?}");
                        crate::tabs::apply_actions(&pb, vec![a]);
                        crate::tabs::emit_all_states(&pb);
                        pb.db.spawn(|db| db.shrink_memory());
                    }
                }
                if os_low {
                    // The notification stays signaled while memory is low; don't spin.
                    std::thread::sleep(Duration::from_secs(5));
                }
            }
            #[allow(unreachable_code)]
            if let Some(h) = low {
                unsafe {
                    let _ = CloseHandle(h);
                }
            }
        })
        .expect("spawn pressure monitor");
}
