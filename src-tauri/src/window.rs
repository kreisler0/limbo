//! The one frameless browser window: Mica on Windows 11, remembered placement,
//! fullscreen, and caption-button helpers.

use limbo_core::sessions::WindowPlacement;
use limbo_core::settings::Settings;
use serde::Serialize;
use tauri::utils::WindowEffect;
use tauri::utils::config::WindowEffectsConfig;
use tauri::window::WindowBuilder;
use tauri::{AppHandle, Manager, PhysicalPosition, PhysicalSize, Window, WindowEvent, Wry};

use crate::state::SharedBrowser;

/// Windows 11 is build 22000+.
pub fn is_windows_11() -> bool {
    use windows::Win32::System::Registry::{HKEY_LOCAL_MACHINE, RRF_RT_REG_SZ, RegGetValueW};
    use windows_core::w;
    let mut buf = [0u16; 32];
    let mut size = (buf.len() * 2) as u32;
    let ok = unsafe {
        RegGetValueW(
            HKEY_LOCAL_MACHINE,
            w!("SOFTWARE\\Microsoft\\Windows NT\\CurrentVersion"),
            w!("CurrentBuildNumber"),
            RRF_RT_REG_SZ,
            None,
            Some(buf.as_mut_ptr() as *mut _),
            Some(&mut size),
        )
    };
    if ok.is_err() {
        return false;
    }
    let s = String::from_utf16_lossy(&buf[..(size as usize / 2).saturating_sub(1)]);
    s.trim_end_matches('\0').parse::<u32>().map(|b| b >= 22000).unwrap_or(false)
}

pub fn create(
    app: &AppHandle<Wry>,
    settings: &Settings,
    placement: Option<WindowPlacement>,
) -> tauri::Result<Window<Wry>> {
    let mica = settings.mica && is_windows_11();
    let mut builder = WindowBuilder::new(app, "main")
        .title("Limbo")
        .decorations(false)
        .shadow(true)
        // Shown after the UI's first paint (no white flash).
        .visible(false)
        .min_inner_size(480.0, 360.0)
        .inner_size(1280.0, 800.0);
    if mica {
        builder = builder
            .transparent(true)
            .effects(WindowEffectsConfig { effects: vec![WindowEffect::Mica], ..Default::default() });
    }
    let window = builder.build()?;
    if let Some(p) = placement.filter(|p| p.width >= 480 && p.height >= 360) {
        let _ = window.set_size(PhysicalSize::new(p.width, p.height));
        if on_screen(&window, p.x, p.y) {
            let _ = window.set_position(PhysicalPosition::new(p.x, p.y));
        } else {
            let _ = window.center();
        }
        if p.maximized {
            let _ = window.maximize();
        }
    } else {
        let _ = window.center();
    }
    Ok(window)
}

/// A saved position on a monitor that's since been unplugged would hide the window.
fn on_screen(window: &Window<Wry>, x: i32, y: i32) -> bool {
    window
        .available_monitors()
        .map(|ms| {
            ms.iter().any(|m| {
                let p = m.position();
                let s = m.size();
                x + 100 >= p.x && y >= p.y - 20 && x < p.x + s.width as i32 - 100 && y < p.y + s.height as i32 - 50
            })
        })
        .unwrap_or(true)
}

#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WindowState {
    pub maximized: bool,
    pub fullscreen: bool,
    pub focused: bool,
    pub minimized: bool,
}

pub fn state(b: &crate::state::Browser) -> WindowState {
    WindowState {
        maximized: b.window.is_maximized().unwrap_or(false),
        fullscreen: b.inner.lock().fullscreen,
        focused: b.window.is_focused().unwrap_or(true),
        minimized: b.window.is_minimized().unwrap_or(false),
    }
}

/// Keeps the page area glued to the window as it resizes.
pub fn watch(b: &SharedBrowser) {
    let weak = std::sync::Arc::downgrade(b);
    b.window.on_window_event(move |event| {
        let Some(b) = weak.upgrade() else { return };
        match event {
            WindowEvent::Resized(_) | WindowEvent::ScaleFactorChanged { .. } => {
                crate::tabs::place_active(&b);
                let st = state(&b);
                let changed = {
                    let mut inner = b.inner.lock();
                    let changed = inner.minimized != st.minimized;
                    inner.minimized = st.minimized;
                    changed
                };
                if changed {
                    // Sampling stops while minimized.
                    b.sampler_signal.notify();
                }
                b.emit("window:state", st);
            }
            WindowEvent::Focused(_) => b.emit("window:state", state(&b)),
            WindowEvent::CloseRequested { .. } => {
                crate::session::save_now(&b);
            }
            _ => {}
        }
    });
}

pub fn set_fullscreen(b: &SharedBrowser, on: bool) {
    b.inner.lock().fullscreen = on;
    let _ = b.window.set_fullscreen(on);
    crate::tabs::place_active(b);
    b.emit("window:state", state(b));
    if !on {
        // Leaving via F11 while a page element is fullscreen: tell the page too.
        if let Some(wv) = b.inner.lock().active_webview() {
            crate::webview2::with_core(&wv, |_, core| {
                crate::webview2::execute_script(&core, "document.fullscreenElement && document.exitFullscreen()")
            });
        }
    }
}

pub fn toggle_maximize(b: &SharedBrowser) {
    if b.window.is_maximized().unwrap_or(false) {
        let _ = b.window.unmaximize();
    } else {
        let _ = b.window.maximize();
    }
}

/// Windows 11 snap layouts. Our maximize button is drawn by the UI webview,
/// which WebView2 renders in its own process, so the window can't answer
/// `WM_NCHITTEST` with `HTMAXBUTTON` there. After hovering the button the UI
/// asks for the flyout, and we open it the way the keyboard does (Win+Z).
pub fn show_snap_layouts(b: &SharedBrowser) {
    use windows::Win32::UI::Input::KeyboardAndMouse::*;
    if !is_windows_11() || !b.window.is_focused().unwrap_or(false) {
        return;
    }
    let key = |vk: VIRTUAL_KEY, up: bool| INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 {
            ki: KEYBDINPUT {
                wVk: vk,
                wScan: 0,
                dwFlags: if up { KEYEVENTF_KEYUP } else { KEYBD_EVENT_FLAGS(0) },
                time: 0,
                dwExtraInfo: 0,
            },
        },
    };
    let inputs = [key(VK_LWIN, false), key(VK_Z, false), key(VK_Z, true), key(VK_LWIN, true)];
    unsafe {
        SendInput(&inputs, std::mem::size_of::<INPUT>() as i32);
    }
}

pub fn show_when_ready(b: &SharedBrowser) {
    let _ = b.window.show();
    let _ = b.window.set_focus();
    if let Some(w) = b.app.get_window("main") {
        let _ = w.set_focus();
    }
}
