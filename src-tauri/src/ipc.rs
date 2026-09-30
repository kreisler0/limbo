//! Commands the UI webview may call (and only the UI webview: see
//! `capabilities/ui.json`; tab webviews get no capabilities). Arguments are
//! camelCase on the JS side. Keep in sync with `ui/src/lib/ipc.ts` and the
//! command list in `build.rs`.

use limbo_core::bookmarks::{self, Bookmark, BookmarkKind};
use limbo_core::db::roots;
use limbo_core::extensions::store::Store;
use limbo_core::history::{self, HistoryEntry, TopSite};
use limbo_core::lifecycle::{Policy, Preset};
use limbo_core::memory::SamplingMode;
use limbo_core::settings::Settings;
use limbo_core::suggest::Suggestion;
use limbo_core::vault::{self, LoginSummary};
use serde::{Deserialize, Serialize};
use tauri::State;
use tauri::ipc::Response;

use crate::state::{Insets, SharedBrowser, TabId, TabInfo};
use crate::tabs::{self, CreateOptions};

type R<T> = Result<T, String>;
type B<'a> = State<'a, SharedBrowser>;

// --- app -----------------------------------------------------------------------

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Platform {
    pub windows11: bool,
    pub mica: bool,
    pub version: &'static str,
    pub engine_version: Option<String>,
    /// Running from a portable package: no default-browser registration.
    pub portable: bool,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppState {
    pub tabs: Vec<TabInfo>,
    pub active_id: Option<TabId>,
    pub settings: Settings,
    pub platform: Platform,
    pub window: crate::window::WindowState,
    pub first_run: bool,
    pub is_default_browser: bool,
}

#[tauri::command]
pub async fn app_state(b: B<'_>) -> R<AppState> {
    let settings = b.settings();
    let win11 = crate::window::is_windows_11();
    Ok(AppState {
        tabs: b.tab_infos(),
        active_id: b.inner.lock().active,
        first_run: !settings.onboarding_done,
        platform: Platform {
            windows11: win11,
            mica: win11 && settings.mica,
            version: env!("CARGO_PKG_VERSION"),
            engine_version: crate::extensions::BROWSER_VERSION.get().cloned(),
            portable: b.paths.portable,
        },
        settings,
        window: crate::window::state(&b),
        is_default_browser: crate::default_browser::is_default(),
    })
}

/// Saves the session and starts Limbo again (engine flags, SmartScreen).
#[tauri::command]
pub async fn app_restart(b: B<'_>) -> R<()> {
    crate::recovery::restart(&b);
    Ok(())
}

/// The UI painted its first frame: show the window (no white flash).
#[tauri::command]
pub async fn app_ready(b: B<'_>) -> R<()> {
    b.inner.lock().ui_ready = true;
    crate::window::show_when_ready(&b);
    if let Some(ui) = b.ui.get() {
        crate::webview2::with_core(ui, |_, core| {
            if let Some(id) = crate::webview2::main_frame_id(&core) {
                crate::UI_FRAME_ID.store(id, std::sync::atomic::Ordering::Relaxed);
            }
        });
    }
    b.sampler_signal.notify();
    Ok(())
}

/// Answers a pending prompt (permission, context menu, save password, ...).
#[tauri::command]
pub async fn prompt_respond(b: B<'_>, id: u64, value: serde_json::Value) -> R<bool> {
    Ok(b.pending.respond(id, value))
}

#[tauri::command]
pub async fn shortcut(b: B<'_>, action: crate::shortcuts::Shortcut) -> R<()> {
    crate::shortcuts::perform(&b, action).await;
    Ok(())
}

// --- tabs ------------------------------------------------------------------------

#[tauri::command]
pub async fn tabs_create(b: B<'_>, url: Option<String>, options: Option<CreateOptions>) -> R<TabId> {
    tabs::create(&b, url, options.unwrap_or_default()).await
}

#[tauri::command]
pub async fn tabs_close(b: B<'_>, id: TabId) -> R<()> {
    tabs::close(&b, id).await;
    Ok(())
}

#[tauri::command]
pub async fn tabs_activate(b: B<'_>, id: TabId, focus_page: Option<bool>) -> R<()> {
    tabs::activate(&b, id, focus_page.unwrap_or(true)).await;
    Ok(())
}

#[tauri::command]
pub async fn tabs_move(b: B<'_>, id: TabId, index: usize) -> R<()> {
    tabs::move_tab(&b, id, index);
    Ok(())
}

#[tauri::command]
pub async fn tabs_pin(b: B<'_>, id: TabId, on: bool) -> R<()> {
    tabs::set_pinned(&b, id, on);
    Ok(())
}

#[tauri::command]
pub async fn tabs_mute(b: B<'_>, id: TabId, on: bool) -> R<()> {
    tabs::set_muted(&b, id, on);
    Ok(())
}

#[tauri::command]
pub async fn tabs_duplicate(b: B<'_>, id: TabId) -> R<Option<TabId>> {
    Ok(tabs::duplicate(&b, id).await)
}

#[tauri::command]
pub async fn tabs_reopen_closed(b: B<'_>) -> R<Option<TabId>> {
    Ok(tabs::reopen_closed(&b).await)
}

/// JPEG of a tab (tab overview, unloaded-tab placeholder).
#[tauri::command]
pub async fn tabs_snapshot(b: B<'_>, id: TabId) -> R<Response> {
    Ok(Response::new(crate::overlay::tab_snapshot(&b, id).await))
}

// --- navigation ----------------------------------------------------------------------

#[tauri::command]
pub async fn nav_go(b: B<'_>, id: TabId, input: String, new_tab: Option<bool>) -> R<()> {
    tabs::go(&b, id, &input, new_tab.unwrap_or(false)).await
}

/// Loads an exact URL (suggestion rows, bookmarks, history).
#[tauri::command]
pub async fn nav_url(b: B<'_>, id: TabId, url: String, typed: Option<bool>) -> R<()> {
    tabs::navigate(&b, id, &url, typed.unwrap_or(false)).await
}

#[tauri::command]
pub async fn nav_back(b: B<'_>, id: TabId) -> R<()> {
    tabs::nav_command(&b, id, "back");
    Ok(())
}

#[tauri::command]
pub async fn nav_forward(b: B<'_>, id: TabId) -> R<()> {
    tabs::nav_command(&b, id, "forward");
    Ok(())
}

#[tauri::command]
pub async fn nav_reload(b: B<'_>, id: TabId, hard: Option<bool>) -> R<()> {
    let crashed = b.inner.lock().tab(id).map(|t| t.info.crashed).unwrap_or(false);
    if crashed {
        // A crashed renderer: rebuild the webview.
        let url = b.inner.lock().tab(id).map(|t| t.info.url.clone()).unwrap_or_default();
        if let Some(wv) = b.inner.lock().tab_mut(id).and_then(|t| t.webview.take()) {
            let _ = wv.close();
        }
        return tabs::navigate(&b, id, &url, false).await;
    }
    if hard.unwrap_or(false) {
        tabs::hard_reload(&b, id)
    } else {
        tabs::nav_command(&b, id, "reload")
    }
    Ok(())
}

#[tauri::command]
pub async fn nav_stop(b: B<'_>, id: TabId) -> R<()> {
    tabs::nav_command(&b, id, "stop");
    Ok(())
}

#[tauri::command]
pub async fn layout_set_insets(b: B<'_>, insets: Insets) -> R<()> {
    b.inner.lock().insets = insets;
    tabs::place_active(&b);
    Ok(())
}

// --- overlays --------------------------------------------------------------------------

#[tauri::command]
pub async fn overlay_capture(b: B<'_>) -> R<Response> {
    Ok(Response::new(crate::overlay::capture(&b).await))
}

#[tauri::command]
pub async fn overlay_hide_content(b: B<'_>) -> R<()> {
    crate::overlay::hide_content(&b);
    Ok(())
}

#[tauri::command]
pub async fn overlay_show_content(b: B<'_>) -> R<()> {
    crate::overlay::show_content(&b);
    Ok(())
}

// --- suggestions, history, new tab ----------------------------------------------------

#[tauri::command]
pub async fn suggest_query(b: B<'_>, text: String, deleting: Option<bool>) -> R<Vec<Suggestion>> {
    crate::suggest::query(&b, text, deleting.unwrap_or(false)).await
}

#[tauri::command]
pub async fn suggest_remove(b: B<'_>, url: String) -> R<()> {
    crate::suggest::remove(&b, url).await
}

#[tauri::command]
pub async fn history_search(
    b: B<'_>,
    query: Option<String>,
    before: Option<i64>,
    limit: Option<usize>,
) -> R<Vec<HistoryEntry>> {
    b.db.run(move |db| history::visits(db.conn(), query.as_deref(), before, limit.unwrap_or(100).min(500))).await
}

#[tauri::command]
pub async fn history_delete(b: B<'_>, visit_ids: Vec<i64>) -> R<()> {
    b.db.run(move |db| history::delete_visits(db.conn_mut(), &visit_ids, limbo_core::db::now_us())).await
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ClearKinds {
    pub history: bool,
    pub cookies: bool,
    pub cache: bool,
    pub downloads: bool,
    pub passwords: bool,
    pub autofill: bool,
}

#[tauri::command]
pub async fn clear_browsing_data(b: B<'_>, since_us: i64, kinds: ClearKinds) -> R<()> {
    let now = limbo_core::db::now_us();
    let k = kinds.clone();
    b.db.run(move |db| {
        if k.history {
            history::clear_range(db.conn_mut(), since_us, i64::MAX, now)?;
        }
        if k.downloads {
            limbo_core::downloads::clear_finished(db.conn())?;
        }
        if k.passwords {
            db.conn().execute("DELETE FROM logins WHERE created_us >= ?1", [since_us])?;
        }
        if k.autofill {
            db.conn().execute("DELETE FROM form_history WHERE COALESCE(last_used_us, 0) >= ?1", [since_us])?;
        }
        Ok(())
    })
    .await?;
    if kinds.cookies || kinds.cache || kinds.autofill || kinds.history {
        crate::browsing_data::clear_engine(&b, since_us, &kinds).await;
    }
    Ok(())
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Tile {
    #[serde(flatten)]
    pub site: TopSite,
    pub favicon: Option<String>,
}

#[tauri::command]
pub async fn top_sites(b: B<'_>, limit: Option<usize>) -> R<Vec<Tile>> {
    b.db.run(move |db| {
        let sites = history::top_sites(db.conn(), limit.unwrap_or(8).min(16))?;
        Ok(sites
            .into_iter()
            .map(|s| {
                let favicon = limbo_core::favicons::for_page(db.conn(), &s.url)
                    .ok()
                    .flatten()
                    .map(|(m, d)| crate::util::data_url(&m, &d));
                Tile { site: s, favicon }
            })
            .collect())
    })
    .await
}

#[tauri::command]
pub async fn favicon_for(b: B<'_>, url: String) -> R<Option<String>> {
    b.db.run(move |db| Ok(limbo_core::favicons::for_page(db.conn(), &url)?.map(|(m, d)| crate::util::data_url(&m, &d))))
        .await
}

// --- bookmarks ---------------------------------------------------------------------------

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BookmarkRoots {
    pub toolbar: Bookmark,
    pub menu: Bookmark,
    pub other: Bookmark,
    pub mobile: Bookmark,
}

#[tauri::command]
pub async fn bookmarks_tree(b: B<'_>) -> R<BookmarkRoots> {
    b.db.run(|db| {
        let c = db.conn();
        let get = |g: &str| bookmarks::tree(c, bookmarks::root_id(c, g)?);
        Ok(BookmarkRoots {
            toolbar: get(roots::TOOLBAR)?,
            menu: get(roots::MENU)?,
            other: get(roots::OTHER)?,
            mobile: get(roots::MOBILE)?,
        })
    })
    .await
}

fn changed(b: &SharedBrowser) {
    b.emit("bookmarks:changed", ());
}

#[tauri::command]
pub async fn bookmarks_add(
    b: B<'_>,
    parent_id: Option<i64>,
    kind: Option<BookmarkKind>,
    title: String,
    url: Option<String>,
    index: Option<i64>,
) -> R<i64> {
    let id =
        b.db.run(move |db| {
            let parent = match parent_id {
                Some(p) => p,
                None => bookmarks::root_id(db.conn(), roots::TOOLBAR)?,
            };
            bookmarks::insert(db.conn(), parent, kind.unwrap_or(BookmarkKind::Url), &title, url.as_deref(), index)
        })
        .await?;
    changed(&b);
    Ok(id)
}

#[tauri::command]
pub async fn bookmarks_update(b: B<'_>, id: i64, title: Option<String>, url: Option<String>) -> R<()> {
    b.db.run(move |db| bookmarks::update(db.conn(), id, title.as_deref(), url.as_deref())).await?;
    changed(&b);
    Ok(())
}

#[tauri::command]
pub async fn bookmarks_move(b: B<'_>, id: i64, parent_id: i64, index: Option<i64>) -> R<()> {
    b.db.run(move |db| bookmarks::move_to(db.conn_mut(), id, parent_id, index)).await?;
    changed(&b);
    Ok(())
}

/// Returns the removed subtree; pass it to `bookmarks_restore` to undo.
#[tauri::command]
pub async fn bookmarks_remove(b: B<'_>, id: i64) -> R<Bookmark> {
    let removed = b.db.run(move |db| bookmarks::remove(db.conn_mut(), id)).await?;
    changed(&b);
    Ok(removed)
}

#[tauri::command]
pub async fn bookmarks_restore(b: B<'_>, subtree: Bookmark) -> R<i64> {
    let id = b.db.run(move |db| bookmarks::restore(db.conn_mut(), &subtree)).await?;
    changed(&b);
    Ok(id)
}

#[tauri::command]
pub async fn bookmarks_for_url(b: B<'_>, url: String) -> R<Vec<Bookmark>> {
    b.db.run(move |db| bookmarks::find_by_url(db.conn(), &url)).await
}

#[tauri::command]
pub async fn bookmarks_import_html(b: B<'_>) -> R<Option<usize>> {
    let Some(path) = crate::dialog::open_file(&b, "Import bookmarks", &[("Bookmarks", "*.html;*.htm")]).await else {
        return Ok(None);
    };
    let html = std::fs::read_to_string(&path).map_err(|e| e.to_string())?;
    let n =
        b.db.run(move |db| {
            let other = bookmarks::root_id(db.conn(), roots::OTHER)?;
            bookmarks::import_html(db.conn_mut(), &html, other)
        })
        .await?;
    changed(&b);
    Ok(Some(n))
}

#[tauri::command]
pub async fn bookmarks_export_html(b: B<'_>) -> R<Option<String>> {
    let Some(path) = crate::dialog::save_file(&b, "Export bookmarks", "bookmarks.html", &[("HTML", "*.html")]).await
    else {
        return Ok(None);
    };
    let html = b.db.run(|db| bookmarks::export_html(db.conn())).await?;
    std::fs::write(&path, html).map_err(|e| e.to_string())?;
    Ok(Some(path.to_string_lossy().into_owned()))
}

// --- passwords ------------------------------------------------------------------------------

#[tauri::command]
pub async fn passwords_list(b: B<'_>) -> R<Vec<LoginSummary>> {
    let p = b.protector.clone();
    b.db.run(move |db| vault::list(db.conn(), &*p, None)).await
}

#[tauri::command]
pub async fn passwords_for_tab(b: B<'_>, id: TabId) -> R<Vec<LoginSummary>> {
    let url = b.inner.lock().tab(id).map(|t| t.info.url.clone()).ok_or("no such tab")?;
    let p = b.protector.clone();
    b.db.run(move |db| vault::for_page(db.conn(), &*p, &url)).await
}

#[tauri::command]
pub async fn passwords_reveal(b: B<'_>, id: i64) -> R<String> {
    crate::passwords::reveal(&b, id).await
}

#[tauri::command]
pub async fn passwords_delete(b: B<'_>, id: i64) -> R<()> {
    b.db.run(move |db| vault::delete(db.conn(), id)).await
}

#[tauri::command]
pub async fn passwords_export(b: B<'_>) -> R<Option<String>> {
    crate::passwords::export(&b).await
}

#[tauri::command]
pub async fn passwords_import_csv(b: B<'_>) -> R<Option<vault::ImportCounts>> {
    crate::passwords::import_csv(&b).await
}

#[tauri::command]
pub async fn autofill_fill(b: B<'_>, tab_id: TabId, login_id: i64) -> R<()> {
    crate::passwords::fill(&b, tab_id, login_id).await
}

// --- extensions --------------------------------------------------------------------------------

#[tauri::command]
pub async fn ext_list(b: B<'_>) -> R<Vec<crate::extensions::ExtensionInfo>> {
    crate::extensions::list(&b).await
}

#[tauri::command]
pub async fn ext_prepare_store(b: B<'_>, store: Store, id: String) -> R<crate::extensions::InstallReview> {
    crate::extensions::prepare_store(&b, store, id).await
}

#[tauri::command]
pub async fn ext_prepare_file(b: B<'_>) -> R<Option<crate::extensions::InstallReview>> {
    crate::extensions::prepare_file(&b).await
}

#[tauri::command]
pub async fn ext_confirm(b: B<'_>, token: u64) -> R<crate::extensions::ExtensionInfo> {
    crate::extensions::confirm(&b, token).await
}

#[tauri::command]
pub async fn ext_load_unpacked(b: B<'_>) -> R<Option<crate::extensions::ExtensionInfo>> {
    crate::extensions::load_unpacked(&b).await
}

#[tauri::command]
pub async fn ext_remove(b: B<'_>, id: String) -> R<()> {
    crate::extensions::remove(&b, id).await
}

#[tauri::command]
pub async fn ext_set_enabled(b: B<'_>, id: String, on: bool) -> R<()> {
    crate::extensions::set_enabled(&b, id, on).await
}

#[tauri::command]
pub async fn ext_set_pinned(b: B<'_>, id: String, on: bool) -> R<()> {
    crate::extensions::set_pinned(&b, id, on).await
}

#[tauri::command]
pub async fn ext_open_popup(b: B<'_>, id: String, popup: String, anchor: crate::extensions::Anchor) -> R<()> {
    crate::extensions::open_popup(&b, id, popup, anchor).await
}

#[tauri::command]
pub async fn ext_close_popup(b: B<'_>) -> R<()> {
    crate::extensions::close_popup(&b);
    Ok(())
}

#[tauri::command]
pub async fn ext_open_options(b: B<'_>, id: String, page: String) -> R<()> {
    crate::extensions::open_options(&b, id, page).await
}

// --- import ---------------------------------------------------------------------------------------

#[tauri::command]
pub async fn import_detect() -> R<Vec<limbo_core::import::firefox::profiles::FirefoxProfile>> {
    tauri::async_runtime::spawn_blocking(crate::import::detect).await.map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn import_choose_folder(b: B<'_>) -> R<Option<limbo_core::import::firefox::profiles::FirefoxProfile>> {
    Ok(crate::import::choose_folder(&b).await)
}

#[tauri::command]
pub async fn import_run(b: B<'_>, args: crate::import::RunArgs) -> R<crate::import::Summary> {
    Ok(crate::import::run(&b, args).await)
}

// --- downloads ----------------------------------------------------------------------------------------

/// Picks the downloads folder and stores it in settings.
#[tauri::command]
pub async fn downloads_choose_folder(b: B<'_>) -> R<Option<Settings>> {
    let Some(dir) = crate::dialog::pick_folder(&b, "Choose a downloads folder").await else { return Ok(None) };
    let patch = serde_json::json!({ "downloadsFolder": dir.to_string_lossy() });
    settings_set(b, patch).await.map(Some)
}

#[tauri::command]
pub async fn downloads_list(b: B<'_>) -> R<Vec<crate::downloads::Progress>> {
    crate::downloads::list(&b).await
}

#[tauri::command]
pub async fn downloads_control(b: B<'_>, id: i64, action: String) -> R<()> {
    let what = match action.as_str() {
        "pause" => "pause",
        "resume" => "resume",
        "cancel" => "cancel",
        _ => return Err("unknown action".into()),
    };
    crate::downloads::control(&b, id, what);
    Ok(())
}

#[tauri::command]
pub async fn downloads_open(path: String) -> R<()> {
    crate::downloads::open_file(&path);
    Ok(())
}

#[tauri::command]
pub async fn downloads_show_in_folder(path: String) -> R<()> {
    crate::downloads::show_in_folder(&path);
    Ok(())
}

#[tauri::command]
pub async fn downloads_remove(b: B<'_>, id: i64) -> R<()> {
    b.db.run(move |db| limbo_core::downloads::remove(db.conn(), id)).await
}

#[tauri::command]
pub async fn downloads_clear(b: B<'_>) -> R<usize> {
    b.db.run(|db| limbo_core::downloads::clear_finished(db.conn())).await
}

// --- site permissions ------------------------------------------------------------------------------------

#[tauri::command]
pub async fn site_permissions(b: B<'_>, origin: Option<String>) -> R<Vec<limbo_core::sites::SitePermission>> {
    b.db.run(move |db| limbo_core::sites::permissions(db.conn(), origin.as_deref())).await
}

#[tauri::command]
pub async fn site_permission_revoke(b: B<'_>, origin: String, kind: String) -> R<()> {
    crate::permissions::forget(&b, origin, kind);
    Ok(())
}

// --- settings ----------------------------------------------------------------------------------------------

#[tauri::command]
pub async fn settings_get(b: B<'_>) -> R<Settings> {
    Ok(b.settings())
}

/// Applies a partial update and returns the new settings.
#[tauri::command]
pub async fn settings_set(b: B<'_>, patch: serde_json::Value) -> R<Settings> {
    let current = b.settings();
    let next = current.merged(&patch).map_err(|e| e.to_string())?;
    let memory_changed = next.memory != current.memory;
    {
        let mut inner = b.inner.lock();
        inner.settings = next.clone();
    }
    if memory_changed {
        crate::memory_commands::apply_policy(&b, next.memory);
    }
    let s = next.clone();
    b.db.run(move |db| db.save_settings(&s)).await?;
    b.sampler_signal.notify();
    b.emit("settings:changed", next.clone());
    Ok(next)
}

// --- find, zoom --------------------------------------------------------------------------------------------

#[tauri::command]
pub async fn find_start(b: B<'_>, id: TabId, text: String, match_case: Option<bool>) -> R<()> {
    crate::find::start(&b, id, text, match_case.unwrap_or(false));
    Ok(())
}

#[tauri::command]
pub async fn find_step(b: B<'_>, id: TabId, forward: bool) -> R<()> {
    crate::find::step(&b, id, forward);
    Ok(())
}

#[tauri::command]
pub async fn find_stop(b: B<'_>, id: TabId) -> R<()> {
    crate::find::stop(&b, id);
    Ok(())
}

#[tauri::command]
pub async fn zoom_step(b: B<'_>, id: TabId, step: crate::zoom::Step) -> R<()> {
    crate::zoom::step(&b, id, step).await;
    Ok(())
}

// --- window ---------------------------------------------------------------------------------------------------

#[tauri::command]
pub async fn window_minimize(b: B<'_>) -> R<()> {
    b.window.minimize().map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn window_toggle_maximize(b: B<'_>) -> R<()> {
    crate::window::toggle_maximize(&b);
    Ok(())
}

#[tauri::command]
pub async fn window_close(b: B<'_>) -> R<()> {
    crate::session::save_now(&b);
    b.window.close().map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn window_toggle_fullscreen(b: B<'_>) -> R<()> {
    let on = !b.inner.lock().fullscreen;
    crate::window::set_fullscreen(&b, on);
    Ok(())
}

#[tauri::command]
pub async fn window_snap_layouts(b: B<'_>) -> R<()> {
    crate::window::show_snap_layouts(&b);
    Ok(())
}

// --- memory -------------------------------------------------------------------------------------------------------

#[tauri::command]
pub async fn memory_set_sampling(b: B<'_>, mode: SamplingMode) -> R<()> {
    b.inner.lock().sampling = mode;
    b.sampler_signal.notify();
    Ok(())
}

#[tauri::command]
pub async fn memory_sleep_tab(b: B<'_>, id: TabId) -> R<()> {
    crate::memory_commands::sleep(&b, id);
    Ok(())
}

#[tauri::command]
pub async fn memory_unload_tab(b: B<'_>, id: TabId) -> R<()> {
    crate::memory_commands::unload(&b, id);
    Ok(())
}

#[tauri::command]
pub async fn memory_sleep_all(b: B<'_>) -> R<()> {
    crate::memory_commands::sleep_all(&b);
    Ok(())
}

#[tauri::command]
pub async fn memory_set_max_awake(b: B<'_>, max: Option<u32>) -> R<Settings> {
    let policy = b.settings().memory.with_max_awake(max);
    settings_set(b, serde_json::json!({ "memory": policy })).await
}

#[tauri::command]
pub async fn memory_set_preset(b: B<'_>, preset: Preset) -> R<Settings> {
    settings_set(b, serde_json::json!({ "memory": Policy::from_preset(preset) })).await
}

// --- misc -------------------------------------------------------------------------------------------------------------

#[tauri::command]
pub async fn print_page(b: B<'_>, id: TabId) -> R<()> {
    crate::browsing_data::print(&b, id);
    Ok(())
}

#[tauri::command]
pub async fn devtools_open(b: B<'_>, id: TabId) -> R<()> {
    if !b.settings().developer_mode {
        return Err("turn on developer mode first".into());
    }
    if let Some(wv) = b.inner.lock().webview(id) {
        crate::webview2::with_core(&wv, |_, core| unsafe {
            let _ = core.OpenDevToolsWindow();
        });
    }
    Ok(())
}

#[tauri::command]
pub async fn default_browser_open(b: B<'_>) -> R<()> {
    // Registering writes to the registry; a portable app must not.
    if b.paths.portable {
        return Err(
            "Portable Limbo can't be the default browser. Install Limbo to use it for links from other apps.".into()
        );
    }
    crate::default_browser::open_settings();
    Ok(())
}

#[tauri::command]
pub async fn default_browser_status() -> R<bool> {
    Ok(crate::default_browser::is_default())
}
