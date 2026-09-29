//! Keyboard shortcuts while a web page has focus. `AcceleratorKeyPressed`
//! sees keys before the page; ours are marked handled, everything else
//! (Ctrl+P, Ctrl+S, page shortcuts) passes through untouched.

use std::sync::Weak;

use serde::{Deserialize, Serialize};
use webview2_com::AcceleratorKeyPressedEventHandler;
use webview2_com::Microsoft::Web::WebView2::Win32::*;
use windows::Win32::UI::Input::KeyboardAndMouse::*;

use crate::state::{Browser, SharedBrowser, TabId};
use crate::tabs::{self, CreateOptions};

/// Actions shared by page-focused (host) and UI-focused (keydown) shortcuts.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Shortcut {
    NewTab,
    NewPrivateTab,
    CloseTab,
    ReopenClosedTab,
    NextTab,
    PrevTab,
    /// Ctrl+1..8 and Ctrl+9 (last).
    Tab1,
    Tab2,
    Tab3,
    Tab4,
    Tab5,
    Tab6,
    Tab7,
    Tab8,
    TabLast,
    Reload,
    HardReload,
    Back,
    Forward,
    ZoomIn,
    ZoomOut,
    ZoomReset,
    Fullscreen,
    // Handled by the UI:
    FocusOmnibox,
    CommandPalette,
    Find,
    FindNext,
    FindPrev,
    Bookmark,
    History,
    Downloads,
    ToggleBookmarksBar,
    ToggleSidebar,
    TabOverview,
    MemoryPopover,
    ClearBrowsingData,
    Escape,
}

impl Shortcut {
    fn handled_by_ui(self) -> bool {
        matches!(
            self,
            Shortcut::FocusOmnibox
                | Shortcut::CommandPalette
                | Shortcut::Find
                | Shortcut::FindNext
                | Shortcut::FindPrev
                | Shortcut::Bookmark
                | Shortcut::History
                | Shortcut::Downloads
                | Shortcut::ToggleBookmarksBar
                | Shortcut::ToggleSidebar
                | Shortcut::TabOverview
                | Shortcut::MemoryPopover
                | Shortcut::ClearBrowsingData
                | Shortcut::Escape
        )
    }
}

fn down(vk: VIRTUAL_KEY) -> bool {
    unsafe { GetKeyState(vk.0 as i32) < 0 }
}

/// Maps a key press to a shortcut. `vk` is the Win32 virtual key.
pub fn classify(vk: u32, ctrl: bool, shift: bool, alt: bool) -> Option<Shortcut> {
    use Shortcut::*;
    let key = VIRTUAL_KEY(vk as u16);
    let letter = |c: u8| vk == c as u32;
    Some(match (ctrl, shift, alt) {
        (true, false, false) => match key {
            _ if letter(b'T') => NewTab,
            _ if letter(b'W') => CloseTab,
            VK_F4 => CloseTab,
            _ if letter(b'L') => FocusOmnibox,
            _ if letter(b'K') => CommandPalette,
            _ if letter(b'E') => FocusOmnibox,
            _ if letter(b'R') => Reload,
            _ if letter(b'F') => Find,
            _ if letter(b'G') => FindNext,
            _ if letter(b'D') => Bookmark,
            _ if letter(b'H') => History,
            _ if letter(b'J') => Downloads,
            _ if letter(b'B') => ToggleSidebar,
            VK_TAB | VK_NEXT => NextTab,
            VK_PRIOR => PrevTab,
            VK_F5 => HardReload,
            VK_OEM_PLUS | VK_ADD => ZoomIn,
            VK_OEM_MINUS | VK_SUBTRACT => ZoomOut,
            _ if letter(b'0') || key == VK_NUMPAD0 => ZoomReset,
            _ if (b'1' as u32..=b'8' as u32).contains(&vk) => match vk as u8 - b'0' {
                1 => Tab1,
                2 => Tab2,
                3 => Tab3,
                4 => Tab4,
                5 => Tab5,
                6 => Tab6,
                7 => Tab7,
                _ => Tab8,
            },
            _ if letter(b'9') => TabLast,
            _ => return None,
        },
        (true, true, false) => match key {
            _ if letter(b'T') => ReopenClosedTab,
            _ if letter(b'N') => NewPrivateTab,
            _ if letter(b'R') => HardReload,
            _ if letter(b'B') => ToggleBookmarksBar,
            _ if letter(b'A') => TabOverview,
            _ if letter(b'M') => MemoryPopover,
            _ if letter(b'G') => FindPrev,
            VK_TAB => PrevTab,
            VK_DELETE => ClearBrowsingData,
            VK_OEM_PLUS => ZoomIn,
            _ => return None,
        },
        (false, false, true) => match key {
            VK_LEFT => Back,
            VK_RIGHT => Forward,
            _ if letter(b'D') => FocusOmnibox,
            _ => return None,
        },
        (false, false, false) => match key {
            VK_F5 => Reload,
            VK_F6 => FocusOmnibox,
            VK_F11 => Fullscreen,
            VK_F3 => FindNext,
            VK_BROWSER_BACK => Back,
            VK_BROWSER_FORWARD => Forward,
            VK_BROWSER_REFRESH => Reload,
            _ => return None,
        },
        (false, true, false) => match key {
            VK_F3 => FindPrev,
            _ => return None,
        },
        _ => return None,
    })
}

pub fn wire(weak: &Weak<Browser>, id: TabId, controller: &ICoreWebView2Controller) -> windows_core::Result<()> {
    let w = weak.clone();
    let mut token = 0i64;
    unsafe {
        controller.add_AcceleratorKeyPressed(
            &AcceleratorKeyPressedEventHandler::create(Box::new(move |_, args| {
                let (Some(b), Some(args)) = (w.upgrade(), args) else { return Ok(()) };
                let mut kind = COREWEBVIEW2_KEY_EVENT_KIND_KEY_DOWN;
                args.KeyEventKind(&mut kind)?;
                if kind != COREWEBVIEW2_KEY_EVENT_KIND_KEY_DOWN && kind != COREWEBVIEW2_KEY_EVENT_KIND_SYSTEM_KEY_DOWN {
                    return Ok(());
                }
                let mut vk = 0u32;
                args.VirtualKey(&mut vk)?;
                let Some(action) = classify(vk, down(VK_CONTROL), down(VK_SHIFT), down(VK_MENU)) else {
                    return Ok(());
                };
                args.SetHandled(true)?;
                let _ = id;
                tauri::async_runtime::spawn(async move { perform(&b, action).await });
                Ok(())
            })),
            &mut token,
        )?;
    }
    Ok(())
}

/// Runs a shortcut. UI-level actions are forwarded to the UI.
pub async fn perform(b: &SharedBrowser, action: Shortcut) {
    use Shortcut::*;
    if action.handled_by_ui() {
        b.emit("shortcut", action);
        return;
    }
    let active = b.inner.lock().active;
    match action {
        NewTab | NewPrivateTab => {
            let private = action == NewPrivateTab;
            if tabs::create(b, None, CreateOptions { private, ..Default::default() }).await.is_ok() {
                b.emit("shortcut", FocusOmnibox);
            }
        }
        CloseTab => {
            if let Some(id) = active {
                tabs::close(b, id).await;
            }
        }
        ReopenClosedTab => {
            tabs::reopen_closed(b).await;
        }
        NextTab | PrevTab => {
            if let Some(id) = tabs::neighbor(b, if action == NextTab { 1 } else { -1 }) {
                tabs::activate(b, id, true).await;
            }
        }
        Tab1 | Tab2 | Tab3 | Tab4 | Tab5 | Tab6 | Tab7 | Tab8 | TabLast => {
            let n = match action {
                Tab1 => 1,
                Tab2 => 2,
                Tab3 => 3,
                Tab4 => 4,
                Tab5 => 5,
                Tab6 => 6,
                Tab7 => 7,
                Tab8 => 8,
                _ => 9,
            };
            if let Some(id) = tabs::nth(b, n) {
                tabs::activate(b, id, true).await;
            }
        }
        Reload => {
            if let Some(id) = active {
                tabs::nav_command(b, id, "reload");
            }
        }
        HardReload => {
            if let Some(id) = active {
                tabs::hard_reload(b, id);
            }
        }
        Back | Forward => {
            if let Some(id) = active {
                tabs::nav_command(b, id, if action == Back { "back" } else { "forward" });
            }
        }
        ZoomIn | ZoomOut | ZoomReset => {
            if let Some(id) = active {
                let dir = match action {
                    ZoomIn => crate::zoom::Step::In,
                    ZoomOut => crate::zoom::Step::Out,
                    _ => crate::zoom::Step::Reset,
                };
                crate::zoom::step(b, id, dir).await;
            }
        }
        Fullscreen => {
            let on = !b.inner.lock().fullscreen;
            crate::window::set_fullscreen(b, on);
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mapping() {
        assert_eq!(classify(b'T' as u32, true, false, false), Some(Shortcut::NewTab));
        assert_eq!(classify(b'T' as u32, true, true, false), Some(Shortcut::ReopenClosedTab));
        assert_eq!(classify(VK_TAB.0 as u32, true, true, false), Some(Shortcut::PrevTab));
        assert_eq!(classify(b'3' as u32, true, false, false), Some(Shortcut::Tab3));
        assert_eq!(classify(b'P' as u32, true, false, false), None, "Ctrl+P stays with the page");
        assert_eq!(classify(VK_LEFT.0 as u32, false, false, true), Some(Shortcut::Back));
    }
}
