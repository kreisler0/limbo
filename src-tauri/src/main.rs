//! Limbo: a lightweight browser for Windows (Tauri 2 + WebView2).
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

#[cfg(not(windows))]
compile_error!("Limbo's host only builds for Windows (WebView2). Use `cargo test` for the core crate.");

mod browsing_data;
mod context_menu;
mod db_worker;
mod default_browser;
mod dialog;
mod downloads;
mod extensions;
mod find;
mod http;
mod import;
mod ipc;
mod logging;
mod memory;
mod memory_commands;
mod overlay;
mod passwords;
mod paths;
mod pending;
mod permissions;
mod popup;
mod recovery;
mod session;
mod shortcuts;
mod state;
mod suggest;
mod tabs;
mod util;
mod webview2;
mod window;
mod zoom;

use std::sync::Arc;
use std::sync::atomic::AtomicU32;

use limbo_core::db::Db;
use tauri::webview::{NewWindowResponse, WebviewBuilder};
use tauri::{LogicalPosition, Manager, RunEvent, WebviewUrl};

use crate::db_worker::DbHandle;
use crate::paths::Paths;
use crate::state::{Browser, SharedBrowser};

/// Main frame id of the UI webview, to tell its renderer apart in memory samples.
pub static UI_FRAME_ID: AtomicU32 = AtomicU32::new(0);

/// URLs and files passed on the command line (default-browser launches).
fn urls_from_args(args: impl IntoIterator<Item = String>) -> Vec<String> {
    args.into_iter()
        .filter(|a| !a.starts_with("--"))
        .filter_map(|a| {
            let lower = a.to_ascii_lowercase();
            if lower.starts_with("http://") || lower.starts_with("https://") || lower.starts_with("file:") {
                Some(a)
            } else if std::path::Path::new(&a).is_file() {
                url::Url::from_file_path(std::fs::canonicalize(&a).ok()?).ok().map(String::from)
            } else {
                None
            }
        })
        .collect()
}

fn open_db(paths: &Paths) -> Db {
    match Db::open(&paths.db) {
        Ok(db) => db,
        Err(e) => {
            // A corrupt database shouldn't keep the browser from starting.
            log::error!("database failed to open ({e}); starting fresh and keeping the old file");
            let backup = paths.db.with_extension(format!("broken-{}.db", limbo_core::db::now_us()));
            let _ = std::fs::rename(&paths.db, backup);
            Db::open(&paths.db).unwrap_or_else(|_| Db::open_in_memory().expect("in-memory database"))
        }
    }
}

/// The UI must never navigate away from its own bundle.
fn ui_navigation_allowed(url: &url::Url) -> bool {
    let host = url.host_str().unwrap_or("");
    url.scheme() == "tauri" || host == "tauri.localhost" || (cfg!(debug_assertions) && host == "localhost")
}

fn main() {
    let paths = Paths::new().expect("create %LOCALAPPDATA%\\Limbo");
    logging::init(paths.logs.clone());
    std::panic::set_hook(Box::new(|info| log::error!("panic: {info}")));
    log::info!("Limbo {} starting", env!("CARGO_PKG_VERSION"));

    let db = open_db(&paths);
    let settings = db.load_settings().unwrap_or_default();
    let session = limbo_core::sessions::load(db.conn()).ok().flatten();
    let _ = limbo_core::downloads::mark_stale_interrupted(db.conn());
    downloads::init(limbo_core::downloads::max_id(db.conn()).unwrap_or(0));
    permissions::init(limbo_core::sites::permissions(db.conn(), None).unwrap_or_default());
    let db = DbHandle::start(db);
    let start_urls = urls_from_args(std::env::args().skip(1));

    let app = tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app, argv, _cwd| {
            let Some(b) = app.try_state::<SharedBrowser>().map(|s| s.inner().clone()) else { return };
            let urls = urls_from_args(argv.into_iter().skip(1));
            let _ = b.window.unminimize();
            let _ = b.window.set_focus();
            tauri::async_runtime::spawn(async move {
                for url in urls {
                    let _ = tabs::create(&b, Some(url), Default::default()).await;
                }
            });
        }))
        .invoke_handler(tauri::generate_handler![
            ipc::app_state,
            ipc::app_ready,
            ipc::prompt_respond,
            ipc::shortcut,
            ipc::tabs_create,
            ipc::tabs_close,
            ipc::tabs_activate,
            ipc::tabs_move,
            ipc::tabs_pin,
            ipc::tabs_mute,
            ipc::tabs_duplicate,
            ipc::tabs_reopen_closed,
            ipc::tabs_snapshot,
            ipc::nav_go,
            ipc::nav_url,
            ipc::nav_back,
            ipc::nav_forward,
            ipc::nav_reload,
            ipc::nav_stop,
            ipc::layout_set_insets,
            ipc::overlay_capture,
            ipc::overlay_hide_content,
            ipc::overlay_show_content,
            ipc::suggest_query,
            ipc::suggest_remove,
            ipc::history_search,
            ipc::history_delete,
            ipc::clear_browsing_data,
            ipc::top_sites,
            ipc::favicon_for,
            ipc::bookmarks_tree,
            ipc::bookmarks_add,
            ipc::bookmarks_update,
            ipc::bookmarks_move,
            ipc::bookmarks_remove,
            ipc::bookmarks_restore,
            ipc::bookmarks_for_url,
            ipc::bookmarks_import_html,
            ipc::bookmarks_export_html,
            ipc::passwords_list,
            ipc::passwords_for_tab,
            ipc::passwords_reveal,
            ipc::passwords_delete,
            ipc::passwords_export,
            ipc::passwords_import_csv,
            ipc::autofill_fill,
            ipc::ext_list,
            ipc::ext_prepare_store,
            ipc::ext_prepare_file,
            ipc::ext_confirm,
            ipc::ext_load_unpacked,
            ipc::ext_remove,
            ipc::ext_set_enabled,
            ipc::ext_set_pinned,
            ipc::ext_open_popup,
            ipc::ext_close_popup,
            ipc::ext_open_options,
            ipc::import_detect,
            ipc::import_choose_folder,
            ipc::import_run,
            ipc::downloads_list,
            ipc::downloads_control,
            ipc::downloads_open,
            ipc::downloads_show_in_folder,
            ipc::downloads_remove,
            ipc::downloads_clear,
            ipc::site_permissions,
            ipc::site_permission_revoke,
            ipc::settings_get,
            ipc::settings_set,
            ipc::find_start,
            ipc::find_step,
            ipc::find_stop,
            ipc::zoom_step,
            ipc::window_minimize,
            ipc::window_toggle_maximize,
            ipc::window_close,
            ipc::window_toggle_fullscreen,
            ipc::window_snap_layouts,
            ipc::memory_set_sampling,
            ipc::memory_sleep_tab,
            ipc::memory_unload_tab,
            ipc::memory_sleep_all,
            ipc::memory_set_max_awake,
            ipc::memory_set_preset,
            ipc::print_page,
            ipc::devtools_open,
            ipc::default_browser_open,
            ipc::default_browser_status,
        ])
        .setup(move |app| {
            let window = window::create(app.handle(), &settings, session.as_ref().and_then(|s| s.window))?;
            let b: SharedBrowser = Arc::new(Browser::new(
                app.handle().clone(),
                window.clone(),
                paths.clone(),
                db.clone(),
                settings.clone(),
            ));
            app.manage(b.clone());

            // The UI webview covers the whole window (transparent); tab
            // webviews sit on top of it in the page area.
            let size = window.inner_size()?.to_logical::<f64>(window.scale_factor()?);
            let ui = window.add_child(
                WebviewBuilder::new("ui", WebviewUrl::App("index.html".into()))
                    .transparent(true)
                    .auto_resize()
                    .data_directory(b.paths.profile.clone())
                    .additional_browser_args(&settings.browser_args())
                    .browser_extensions_enabled(true)
                    .zoom_hotkeys_enabled(false)
                    .on_navigation(ui_navigation_allowed)
                    .on_new_window(|_, _| NewWindowResponse::Deny),
                LogicalPosition::new(0.0, 0.0),
                size,
            )?;
            let _ = b.ui.set(ui.clone());

            // Share this environment with every tab (one browser process).
            let b2 = b.clone();
            ui.with_webview(move |pw| {
                let env = pw.environment();
                unsafe {
                    let mut v = windows_core::PWSTR::null();
                    if env.BrowserVersionString(&mut v).is_ok() {
                        let _ = extensions::BROWSER_VERSION.set(webview2::take_string(v));
                    }
                }
                let _ = b2.env.set(webview2::SendEnv(env));
                if let Ok(core) = unsafe { pw.controller().CoreWebView2() } {
                    webview2::apply_profile_settings(&core);
                }
            })?;

            window::watch(&b);
            tabs::lifecycle::start(&b);
            memory::start(&b);
            extensions::start_updater(&b);

            let b3 = b.clone();
            let session = session.clone();
            let urls = start_urls.clone();
            tauri::async_runtime::spawn(async move {
                session::restore(&b3, session, urls).await;
            });
            // If the UI never reports ready (e.g. a broken bundle), show anyway.
            let b4 = b.clone();
            tauri::async_runtime::spawn(async move {
                tokio::time::sleep(std::time::Duration::from_secs(4)).await;
                if !b4.inner.lock().ui_ready {
                    window::show_when_ready(&b4);
                }
            });
            Ok(())
        })
        .build(tauri::generate_context!())
        .expect("failed to build Limbo");

    app.run(|app, event| {
        if let RunEvent::ExitRequested { .. } = event
            && let Some(b) = app.try_state::<SharedBrowser>()
        {
            let s = session::snapshot(&b);
            let _ = b.db.call_blocking(move |db| limbo_core::sessions::save(db.conn(), &s, limbo_core::db::now_us()));
        }
    });
}
