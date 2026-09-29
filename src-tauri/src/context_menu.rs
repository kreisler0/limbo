//! Our own context menu. WebView2's menu is suppressed (`Handled = true`); the
//! UI draws a curated list and the choice is either one of WebView2's own
//! commands (`SelectedCommandId`, so copy/paste/save-image work natively) or
//! one of ours (open link in new tab, search Google for the selection).

use std::sync::Weak;

use serde::Serialize;
use webview2_com::ContextMenuRequestedEventHandler;
use webview2_com::Microsoft::Web::WebView2::Win32::*;
use windows::Win32::Foundation::POINT;
use windows_core::{BOOL, Interface, PWSTR};

use crate::pending::ThreadBound;
use crate::state::{Browser, SharedBrowser, TabId};
use crate::tabs::{self, CreateOptions};
use crate::webview2::take_string;

const CUSTOM_BASE: i32 = 1_000_000;
const OPEN_LINK_NEW_TAB: i32 = CUSTOM_BASE + 1;
const OPEN_IMAGE_NEW_TAB: i32 = CUSTOM_BASE + 2;
const SEARCH_GOOGLE: i32 = CUSTOM_BASE + 3;
const OPEN_LINK_PRIVATE: i32 = CUSTOM_BASE + 4;

/// WebView2 command names we show, per context.
const LINK: &[&str] = &["copyLinkLocation", "saveLinkAs"];
const IMAGE: &[&str] = &["copyImage", "copyImageLocation", "saveImageAs"];
const MEDIA: &[&str] = &["loop", "showAllControls", "pictureInPicture", "saveMediaAs", "copyVideoLocation"];
const EDIT: &[&str] = &["undo", "redo", "cut", "copy", "paste", "pasteAndMatchStyle", "selectAll", "emoji"];
const SELECTION: &[&str] = &["copy"];
const PAGE: &[&str] = &["back", "forward", "reload", "saveAs", "print"];
const DEV: &[&str] = &["inspectElement"];

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MenuItem {
    pub id: i32,
    pub label: String,
    /// item | checkbox | separator
    pub kind: &'static str,
    pub enabled: bool,
    pub checked: bool,
    pub shortcut: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MenuRequest {
    pub menu_id: u64,
    pub tab_id: TabId,
    /// Position inside the page area, in physical pixels.
    pub x: i32,
    pub y: i32,
    pub items: Vec<MenuItem>,
}

struct Target {
    link: Option<String>,
    source: Option<String>,
    selection: Option<String>,
    editable: bool,
    kind: COREWEBVIEW2_CONTEXT_MENU_TARGET_KIND,
}

unsafe fn opt_string(
    has: impl FnOnce(&mut BOOL) -> windows_core::Result<()>,
    get: impl FnOnce(&mut PWSTR) -> windows_core::Result<()>,
) -> Option<String> {
    let mut b = BOOL::default();
    has(&mut b).ok()?;
    if !b.as_bool() {
        return None;
    }
    let mut p = PWSTR::null();
    get(&mut p).ok()?;
    Some(take_string(p)).filter(|s| !s.is_empty())
}

unsafe fn read_target(t: &ICoreWebView2ContextMenuTarget) -> Target {
    unsafe {
        let mut editable = BOOL::default();
        let _ = t.IsEditable(&mut editable);
        let mut kind = COREWEBVIEW2_CONTEXT_MENU_TARGET_KIND_PAGE;
        let _ = t.Kind(&mut kind);
        Target {
            link: opt_string(|b| t.HasLinkUri(b), |p| t.LinkUri(p)),
            source: opt_string(|b| t.HasSourceUri(b), |p| t.SourceUri(p)),
            selection: opt_string(|b| t.HasSelection(b), |p| t.SelectionText(p)),
            editable: editable.as_bool(),
            kind,
        }
    }
}

struct DefaultItem {
    name: String,
    item: MenuItem,
}

unsafe fn defaults(items: &ICoreWebView2ContextMenuItemCollection) -> Vec<DefaultItem> {
    let mut out = Vec::new();
    unsafe {
        let mut count = 0u32;
        let _ = items.Count(&mut count);
        for i in 0..count {
            let Ok(item) = items.GetValueAtIndex(i) else { continue };
            let mut p = PWSTR::null();
            let _ = item.Name(&mut p);
            let name = take_string(p);
            let mut p = PWSTR::null();
            let _ = item.Label(&mut p);
            let label = take_string(p).replace('&', "");
            let mut id = 0i32;
            let _ = item.CommandId(&mut id);
            let mut kind = COREWEBVIEW2_CONTEXT_MENU_ITEM_KIND_COMMAND;
            let _ = item.Kind(&mut kind);
            let mut enabled = BOOL::default();
            let _ = item.IsEnabled(&mut enabled);
            let mut checked = BOOL::default();
            let _ = item.IsChecked(&mut checked);
            let mut p = PWSTR::null();
            let _ = item.ShortcutKeyDescription(&mut p);
            let shortcut = Some(take_string(p)).filter(|s| !s.is_empty());
            let kind = match kind {
                COREWEBVIEW2_CONTEXT_MENU_ITEM_KIND_SEPARATOR => "separator",
                COREWEBVIEW2_CONTEXT_MENU_ITEM_KIND_CHECK_BOX => "checkbox",
                _ => "item",
            };
            out.push(DefaultItem {
                name,
                item: MenuItem { id, label, kind, enabled: enabled.as_bool(), checked: checked.as_bool(), shortcut },
            });
        }
    }
    out
}

fn separator() -> MenuItem {
    MenuItem { id: 0, label: String::new(), kind: "separator", enabled: false, checked: false, shortcut: None }
}

fn custom(id: i32, label: String) -> MenuItem {
    MenuItem { id, label, kind: "item", enabled: true, checked: false, shortcut: None }
}

fn build(target: &Target, defaults: &[DefaultItem], developer: bool) -> Vec<MenuItem> {
    let mut groups: Vec<Vec<MenuItem>> = Vec::new();
    let pick = |names: &[&str]| -> Vec<MenuItem> {
        names.iter().filter_map(|n| defaults.iter().find(|d| d.name == *n).map(|d| d.item.clone())).collect()
    };
    if target.link.is_some() {
        let mut g = vec![
            custom(OPEN_LINK_NEW_TAB, "Open link in new tab".into()),
            custom(OPEN_LINK_PRIVATE, "Open link in private tab".into()),
        ];
        g.extend(pick(LINK));
        groups.push(g);
    }
    if target.kind == COREWEBVIEW2_CONTEXT_MENU_TARGET_KIND_IMAGE {
        let mut g = vec![custom(OPEN_IMAGE_NEW_TAB, "Open image in new tab".into())];
        g.extend(pick(IMAGE));
        groups.push(g);
    }
    if target.kind == COREWEBVIEW2_CONTEXT_MENU_TARGET_KIND_VIDEO
        || target.kind == COREWEBVIEW2_CONTEXT_MENU_TARGET_KIND_AUDIO
    {
        groups.push(pick(MEDIA));
    }
    if target.editable {
        // Spelling suggestions come first in WebView2's own list; keep them.
        let spelling: Vec<MenuItem> = defaults
            .iter()
            .take_while(|d| d.item.kind != "separator")
            .filter(|d| d.name == "other" || d.name.starts_with("spell"))
            .map(|d| d.item.clone())
            .collect();
        if !spelling.is_empty() {
            groups.push(spelling);
        }
        groups.push(pick(EDIT));
    } else if let Some(sel) = &target.selection {
        let mut g = pick(SELECTION);
        let short: String = sel.chars().take(24).collect();
        let ell = if sel.chars().count() > 24 { "…" } else { "" };
        g.push(custom(SEARCH_GOOGLE, format!("Search Google for \u{201c}{}{ell}\u{201d}", short.trim())));
        groups.push(g);
    }
    if target.link.is_none()
        && target.selection.is_none()
        && !target.editable
        && target.kind == COREWEBVIEW2_CONTEXT_MENU_TARGET_KIND_PAGE
    {
        groups.push(pick(PAGE));
    }
    if developer {
        groups.push(pick(DEV));
    }
    let mut out = Vec::new();
    for g in groups.into_iter().filter(|g| !g.is_empty()) {
        if !out.is_empty() {
            out.push(separator());
        }
        out.extend(g);
    }
    out
}

pub fn wire(weak: &Weak<Browser>, id: TabId, core: &ICoreWebView2) -> windows_core::Result<()> {
    let Ok(c11) = core.cast::<ICoreWebView2_11>() else { return Ok(()) };
    let w = weak.clone();
    let mut token = 0i64;
    unsafe {
        c11.add_ContextMenuRequested(
            &ContextMenuRequestedEventHandler::create(Box::new(move |_, args| {
                let (Some(b), Some(args)) = (w.upgrade(), args) else { return Ok(()) };
                let target = read_target(&args.ContextMenuTarget()?);
                let defaults = defaults(&args.MenuItems()?);
                let items = build(&target, &defaults, b.settings().developer_mode);
                if items.is_empty() {
                    return Ok(());
                }
                let mut pt = POINT::default();
                let _ = args.Location(&mut pt);
                args.SetHandled(true)?;
                let deferral = args.GetDeferral()?;
                let held = ThreadBound::new((args.clone(), deferral));
                let b2 = b.clone();
                let menu_id = b.pending.add(Box::new(move |answer| {
                    let chosen = answer.get("itemId").and_then(|v| v.as_i64()).map(|v| v as i32);
                    let b3 = b2.clone();
                    let _ = b2.app.run_on_main_thread(move || {
                        let Some((args, deferral)) = held.into_inner() else { return };
                        if let Some(cmd) = chosen {
                            if cmd < CUSTOM_BASE {
                                let _ = args.SetSelectedCommandId(cmd);
                            } else {
                                run_custom(&b3, id, cmd, &target);
                            }
                        }
                        let _ = deferral.Complete();
                    });
                }));
                b.emit("context-menu", MenuRequest { menu_id, tab_id: id, x: pt.x, y: pt.y, items });
                Ok(())
            })),
            &mut token,
        )?;
    }
    Ok(())
}

fn run_custom(b: &SharedBrowser, opener: TabId, cmd: i32, target: &Target) {
    let (url, private, background) = match cmd {
        OPEN_LINK_NEW_TAB => (target.link.clone(), false, true),
        OPEN_LINK_PRIVATE => (target.link.clone(), true, false),
        OPEN_IMAGE_NEW_TAB => (target.source.clone(), false, true),
        SEARCH_GOOGLE => (target.selection.as_deref().map(limbo_core::omnibox::google_search_url), false, false),
        _ => (None, false, false),
    };
    let Some(url) = url else { return };
    let b = b.clone();
    tauri::async_runtime::spawn(async move {
        let opts = CreateOptions { opener: Some(opener), background, private, ..Default::default() };
        let _ = tabs::create(&b, Some(url), opts).await;
    });
}
