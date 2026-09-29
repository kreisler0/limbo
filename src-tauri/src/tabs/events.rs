//! WebView2 events for a tab webview. Handlers run on the UI thread; they
//! update the tab under the lock, release it, then emit to the UI.

use std::sync::{Arc, Weak};

use limbo_core::frecency::Transition;
use limbo_core::settings::Settings;
use tauri::{Webview, Wry};
use webview2_com::Microsoft::Web::WebView2::Win32::*;
use webview2_com::*;
use windows_core::{BOOL, Interface};

use crate::state::{Browser, SharedBrowser, TabId};
use crate::webview2;

type W = Weak<Browser>;

fn update(b: &Browser, id: TabId, f: impl FnOnce(&mut crate::state::Tab)) {
    if let Some(t) = b.inner.lock().tab_mut(id) {
        f(t);
    }
    b.emit_tab(id);
}

pub fn wire(b: &SharedBrowser, id: TabId, wv: &Webview<Wry>, settings: &Settings) {
    let weak: W = Arc::downgrade(b);
    let smartscreen = settings.smartscreen_enabled;
    let devtools = settings.developer_mode;
    let site_zoom_url = b.inner.lock().tab(id).map(|t| t.info.url.clone());
    webview2::with_core(wv, move |controller, core| {
        webview2::apply_tab_settings(&core, smartscreen, devtools);
        if let Some(b) = weak.upgrade() {
            let (muted, zoom) = b.inner.lock().tab(id).map(|t| (t.info.muted, t.info.zoom)).unwrap_or((false, 1.0));
            if muted {
                webview2::set_muted(&core, true);
            }
            if (zoom - 1.0).abs() > 0.001 {
                webview2::set_zoom(&controller, zoom);
            }
        }
        if let Err(e) = unsafe { wire_core(&weak, id, &controller, &core) } {
            log::warn!("wiring tab {id} events failed: {e}");
        }
    });
    // Site zoom from the database.
    if let Some(url) = site_zoom_url {
        crate::zoom::apply_saved(b, id, &url);
    }
}

unsafe fn wire_core(
    weak: &W,
    id: TabId,
    controller: &ICoreWebView2Controller,
    core: &ICoreWebView2,
) -> windows_core::Result<()> {
    let mut token = 0i64;

    // --- navigation and progress ------------------------------------------
    let w = weak.clone();
    unsafe {
        core.add_NavigationStarting(
            &NavigationStartingEventHandler::create(Box::new(move |_, args| {
                let (Some(b), Some(args)) = (w.upgrade(), args) else { return Ok(()) };
                let mut redirected = BOOL::default();
                let _ = args.IsRedirected(&mut redirected);
                let mut kind = COREWEBVIEW2_NAVIGATION_KIND_NEW_DOCUMENT;
                if let Ok(a3) = args.cast::<ICoreWebView2NavigationStartingEventArgs3>() {
                    let _ = a3.NavigationKind(&mut kind);
                }
                update(&b, id, |t| {
                    let transition = if kind == COREWEBVIEW2_NAVIGATION_KIND_RELOAD {
                        Transition::Reload
                    } else if redirected.as_bool() {
                        Transition::RedirectTemporary
                    } else if t.typed_navigation {
                        Transition::Typed
                    } else {
                        Transition::Link
                    };
                    t.pending_transition = Some(transition);
                    t.info.loading = true;
                    t.info.progress = 0.1;
                    t.info.crashed = false;
                });
                Ok(())
            })),
            &mut token,
        )?;
    }

    let w = weak.clone();
    unsafe {
        core.add_ContentLoading(
            &ContentLoadingEventHandler::create(Box::new(move |sender, _| {
                let Some(b) = w.upgrade() else { return Ok(()) };
                let frame = sender.as_ref().and_then(webview2::main_frame_id);
                update(&b, id, |t| {
                    t.info.progress = t.info.progress.max(0.4);
                    if frame.is_some_and(|f| f != 0) {
                        t.main_frame_id = frame;
                    }
                });
                Ok(())
            })),
            &mut token,
        )?;
    }

    if let Ok(core2) = core.cast::<ICoreWebView2_2>() {
        let w = weak.clone();
        unsafe {
            core2.add_DOMContentLoaded(
                &DOMContentLoadedEventHandler::create(Box::new(move |_, _| {
                    if let Some(b) = w.upgrade() {
                        update(&b, id, |t| t.info.progress = t.info.progress.max(0.75));
                    }
                    Ok(())
                })),
                &mut token,
            )?;
        }
    }

    let w = weak.clone();
    unsafe {
        core.add_NavigationCompleted(
            &NavigationCompletedEventHandler::create(Box::new(move |sender, args| {
                let (Some(b), Some(args), Some(sender)) = (w.upgrade(), args, sender) else { return Ok(()) };
                let mut ok = BOOL::default();
                let _ = args.IsSuccess(&mut ok);
                let url = webview2::source(&sender);
                let title = webview2::title(&sender);
                let (back, fwd) = webview2::history_state(&sender);
                let mut record: Option<(Transition, bool)> = None;
                update(&b, id, |t| {
                    t.info.loading = false;
                    t.info.progress = 1.0;
                    t.info.can_go_back = back;
                    t.info.can_go_forward = fwd;
                    let transition = t.pending_transition.take().unwrap_or(Transition::Link);
                    t.typed_navigation = false;
                    if ok.as_bool() {
                        record = Some((transition, t.info.private));
                    }
                });
                if let Some((transition, false)) = record {
                    b.db.spawn(move |db| {
                        let title = (!title.is_empty()).then_some(title.as_str());
                        if let Err(e) = limbo_core::history::record_visit(
                            db.conn_mut(),
                            &url,
                            title,
                            transition,
                            limbo_core::db::now_us(),
                        ) {
                            log::warn!("history: {e}");
                        }
                    });
                }
                Ok(())
            })),
            &mut token,
        )?;
    }

    let w = weak.clone();
    unsafe {
        core.add_SourceChanged(
            &SourceChangedEventHandler::create(Box::new(move |sender, _| {
                let (Some(b), Some(sender)) = (w.upgrade(), sender) else { return Ok(()) };
                let url = webview2::source(&sender);
                if url.is_empty() {
                    return Ok(());
                }
                update(&b, id, |t| t.info.set_url(&url));
                crate::zoom::apply_saved(&b, id, &url);
                crate::session::schedule_save(&b);
                Ok(())
            })),
            &mut token,
        )?;
    }

    let w = weak.clone();
    unsafe {
        core.add_HistoryChanged(
            &HistoryChangedEventHandler::create(Box::new(move |sender, _| {
                let (Some(b), Some(sender)) = (w.upgrade(), sender) else { return Ok(()) };
                let (back, fwd) = webview2::history_state(&sender);
                update(&b, id, |t| {
                    t.info.can_go_back = back;
                    t.info.can_go_forward = fwd;
                });
                Ok(())
            })),
            &mut token,
        )?;
    }

    let w = weak.clone();
    unsafe {
        core.add_DocumentTitleChanged(
            &DocumentTitleChangedEventHandler::create(Box::new(move |sender, _| {
                let (Some(b), Some(sender)) = (w.upgrade(), sender) else { return Ok(()) };
                let title = webview2::title(&sender);
                let url = webview2::source(&sender);
                let mut private = false;
                update(&b, id, |t| {
                    private = t.info.private;
                    if t.info.internal.is_none() {
                        t.info.title = title.clone();
                    }
                });
                if !private && !title.is_empty() {
                    b.db.spawn(move |db| {
                        let _ = limbo_core::history::set_title(db.conn(), &url, &title);
                    });
                }
                crate::session::schedule_save(&b);
                Ok(())
            })),
            &mut token,
        )?;
    }

    // --- favicon -----------------------------------------------------------
    if let Ok(c15) = core.cast::<ICoreWebView2_15>() {
        let w = weak.clone();
        unsafe {
            c15.add_FaviconChanged(
                &FaviconChangedEventHandler::create(Box::new(move |sender, _| {
                    let (Some(_), Some(sender)) = (w.upgrade(), sender) else { return Ok(()) };
                    let icon_uri = webview2::favicon_uri(&sender);
                    let page = webview2::source(&sender);
                    let w2 = w.clone();
                    webview2::get_favicon(&sender, move |bytes| {
                        let Some(b) = w2.upgrade() else { return };
                        let Some(bytes) = bytes else {
                            update(&b, id, |t| t.info.favicon = None);
                            return;
                        };
                        let data_url = crate::util::data_url("image/png", &bytes);
                        let mut private = false;
                        update(&b, id, |t| {
                            private = t.info.private;
                            t.info.favicon = Some(data_url);
                        });
                        if !private {
                            let width = crate::util::png_size(&bytes).map(|s| s.0).unwrap_or(0);
                            let icon = icon_uri.unwrap_or_else(|| format!("{page}#favicon"));
                            b.db.spawn(move |db| {
                                let _ = limbo_core::favicons::put(
                                    db.conn(),
                                    &page,
                                    &icon,
                                    &bytes,
                                    width,
                                    limbo_core::db::now_us(),
                                );
                            });
                        }
                    });
                    Ok(())
                })),
                &mut token,
            )?;
        }
    }

    // --- audio ---------------------------------------------------------------
    if let Ok(c8) = core.cast::<ICoreWebView2_8>() {
        let w = weak.clone();
        unsafe {
            c8.add_IsDocumentPlayingAudioChanged(
                &IsDocumentPlayingAudioChangedEventHandler::create(Box::new(move |sender, _| {
                    let (Some(b), Some(sender)) = (w.upgrade(), sender) else { return Ok(()) };
                    let audible = webview2::is_playing_audio(&sender);
                    let flags = {
                        let mut inner = b.inner.lock();
                        let Some(t) = inner.tab_mut(id) else { return Ok(()) };
                        t.info.audible = audible;
                        t.flags.audible = audible;
                        t.flags
                    };
                    let now = b.now_ms();
                    let actions = b.inner.lock().lifecycle.set_flags(id, flags, now);
                    crate::tabs::apply_actions(&b, actions);
                    b.lifecycle_signal.notify();
                    b.emit_tab(id);
                    Ok(())
                })),
                &mut token,
            )?;
        }
    }

    // --- fullscreen ----------------------------------------------------------
    let w = weak.clone();
    unsafe {
        core.add_ContainsFullScreenElementChanged(
            &ContainsFullScreenElementChangedEventHandler::create(Box::new(move |sender, _| {
                let (Some(b), Some(sender)) = (w.upgrade(), sender) else { return Ok(()) };
                let mut on = BOOL::default();
                let _ = sender.ContainsFullScreenElement(&mut on);
                let on = on.as_bool();
                tauri::async_runtime::spawn(async move {
                    crate::window::set_fullscreen(&b, on);
                });
                Ok(())
            })),
            &mut token,
        )?;
    }

    // --- crashes -------------------------------------------------------------
    let w = weak.clone();
    unsafe {
        core.add_ProcessFailed(
            &ProcessFailedEventHandler::create(Box::new(move |_, args| {
                let (Some(b), Some(args)) = (w.upgrade(), args) else { return Ok(()) };
                let mut kind = COREWEBVIEW2_PROCESS_FAILED_KIND_RENDER_PROCESS_EXITED;
                let _ = args.ProcessFailedKind(&mut kind);
                if kind == COREWEBVIEW2_PROCESS_FAILED_KIND_BROWSER_PROCESS_EXITED {
                    crate::recovery::browser_process_exited(&b);
                } else if kind == COREWEBVIEW2_PROCESS_FAILED_KIND_RENDER_PROCESS_EXITED
                    || kind == COREWEBVIEW2_PROCESS_FAILED_KIND_RENDER_PROCESS_UNRESPONSIVE
                {
                    let wv = b.inner.lock().webview(id);
                    update(&b, id, |t| {
                        t.info.crashed = true;
                        t.info.loading = false;
                    });
                    // Hide the dead page so the UI's "This page crashed" shows.
                    if let Some(wv) = wv {
                        let _ = wv.hide();
                    }
                }
                Ok(())
            })),
            &mut token,
        )?;
    }

    // --- zoom ------------------------------------------------------------------
    let w = weak.clone();
    unsafe {
        controller.add_ZoomFactorChanged(
            &ZoomFactorChangedEventHandler::create(Box::new(move |sender, _| {
                let (Some(b), Some(sender)) = (w.upgrade(), sender) else { return Ok(()) };
                let mut z = 1.0f64;
                let _ = sender.ZoomFactor(&mut z);
                update(&b, id, |t| t.info.zoom = z);
                Ok(())
            })),
            &mut token,
        )?;
    }

    // --- focus: clicking into the page closes UI popovers ---------------------
    let w = weak.clone();
    unsafe {
        controller.add_GotFocus(
            &FocusChangedEventHandler::create(Box::new(move |_, _| {
                if let Some(b) = w.upgrade() {
                    b.emit("page:focus", serde_json::json!({ "id": id }));
                }
                Ok(())
            })),
            &mut token,
        )?;
    }

    crate::shortcuts::wire(weak, id, controller)?;
    crate::context_menu::wire(weak, id, core)?;
    crate::downloads::wire(weak, id, core)?;
    crate::permissions::wire(weak, id, core)?;
    crate::passwords::wire(weak, id, core)?;
    crate::find::wire(weak, id, core)?;
    Ok(())
}
