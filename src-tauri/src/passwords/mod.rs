//! Limbo's password vault on Windows: DPAPI encryption, Windows Hello before
//! revealing or exporting, and the autofill protocol with the page script.
//!
//! Message rules: only the top-level document can post (subframe messages
//! never reach `ICoreWebView2::WebMessageReceived`); the sender's origin comes
//! from `args.Source()` and must equal the tab's current origin; unknown
//! messages are dropped; each tab is rate-limited.

mod hello;

use std::collections::HashMap;
use std::sync::Weak;
use std::time::Instant;

use limbo_core::vault::{self, NewLogin, SavePrompt, SecretProtector};
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use webview2_com::Microsoft::Web::WebView2::Win32::*;
use webview2_com::WebMessageReceivedEventHandler;
use windows::Win32::Foundation::{HLOCAL, LocalFree};
use windows::Win32::Security::Cryptography::{
    CRYPT_INTEGER_BLOB, CRYPTPROTECT_UI_FORBIDDEN, CryptProtectData, CryptUnprotectData,
};
use windows_core::{HSTRING, PWSTR};
use zeroize::Zeroizing;

use crate::state::{Browser, SharedBrowser, TabId};
use crate::webview2;

pub use hello::verify_user;

pub const AUTOFILL_JS: &str = include_str!("autofill.js");

const ENTROPY: &[u8] = b"Limbo password vault v1";

/// DPAPI (`CryptProtectData`, current user) with app-specific entropy.
pub struct Protector;

impl Protector {
    pub fn new() -> Protector {
        Protector
    }
}

fn blob(data: &[u8]) -> CRYPT_INTEGER_BLOB {
    CRYPT_INTEGER_BLOB { cbData: data.len() as u32, pbData: data.as_ptr() as *mut u8 }
}

impl SecretProtector for Protector {
    fn protect(&self, plaintext: &[u8]) -> limbo_core::Result<Vec<u8>> {
        let input = blob(plaintext);
        let entropy = blob(ENTROPY);
        let mut out = CRYPT_INTEGER_BLOB::default();
        unsafe {
            CryptProtectData(&input, None, Some(&entropy), None, None, CRYPTPROTECT_UI_FORBIDDEN, &mut out)
                .map_err(|e| limbo_core::Error::Protect(e.to_string()))?;
            let v = std::slice::from_raw_parts(out.pbData, out.cbData as usize).to_vec();
            let _ = LocalFree(Some(HLOCAL(out.pbData as *mut _)));
            Ok(v)
        }
    }

    fn unprotect(&self, ciphertext: &[u8]) -> limbo_core::Result<Zeroizing<Vec<u8>>> {
        let input = blob(ciphertext);
        let entropy = blob(ENTROPY);
        let mut out = CRYPT_INTEGER_BLOB::default();
        unsafe {
            CryptUnprotectData(&input, None, Some(&entropy), None, None, CRYPTPROTECT_UI_FORBIDDEN, &mut out)
                .map_err(|e| limbo_core::Error::Protect(e.to_string()))?;
            let slice = std::slice::from_raw_parts_mut(out.pbData, out.cbData as usize);
            let v = Zeroizing::new(slice.to_vec());
            slice.fill(0);
            let _ = LocalFree(Some(HLOCAL(out.pbData as *mut _)));
            Ok(v)
        }
    }
}

#[derive(Deserialize)]
struct PageMessage {
    limbo: u8,
    t: String,
    #[serde(default)]
    u: Option<String>,
    #[serde(default)]
    p: Option<String>,
    #[serde(default)]
    count: Option<u32>,
}

#[derive(Default)]
struct RateLimit {
    window_start: Option<Instant>,
    count: u32,
}

static RATE: Mutex<Option<HashMap<TabId, RateLimit>>> = Mutex::new(None);

fn allowed(id: TabId) -> bool {
    let mut map = RATE.lock();
    let entry = map.get_or_insert_with(HashMap::new).entry(id).or_default();
    let now = Instant::now();
    match entry.window_start {
        Some(start) if now.duration_since(start).as_secs() < 1 => {
            entry.count += 1;
            entry.count <= 20
        }
        _ => {
            entry.window_start = Some(now);
            entry.count = 1;
            true
        }
    }
}

pub fn wire(weak: &Weak<Browser>, id: TabId, core: &ICoreWebView2) -> windows_core::Result<()> {
    let w = weak.clone();
    let mut token = 0i64;
    unsafe {
        core.add_WebMessageReceived(
            &WebMessageReceivedEventHandler::create(Box::new(move |sender, args| {
                let (Some(b), Some(args), Some(sender)) = (w.upgrade(), args, sender) else { return Ok(()) };
                let mut raw = PWSTR::null();
                if args.TryGetWebMessageAsString(&mut raw).is_err() {
                    return Ok(());
                }
                let text = webview2::take_string(raw);
                // Tauri's own IPC shares this channel; anything that isn't ours is ignored.
                let Ok(msg) = serde_json::from_str::<PageMessage>(&text) else { return Ok(()) };
                if msg.limbo != 1 || !allowed(id) {
                    return Ok(());
                }
                let mut src = PWSTR::null();
                let _ = args.Source(&mut src);
                let source = webview2::take_string(src);
                let current = webview2::source(&sender);
                let (Some(from), Some(page)) =
                    (limbo_core::omnibox::origin_of(&source), limbo_core::omnibox::origin_of(&current))
                else {
                    return Ok(());
                };
                if from != page {
                    return Ok(());
                }
                handle(&b, id, &current, msg);
                Ok(())
            })),
            &mut token,
        )?;
    }
    Ok(())
}

fn handle(b: &SharedBrowser, id: TabId, page_url: &str, msg: PageMessage) {
    match msg.t.as_str() {
        "forms" => announce(b, id, page_url.to_string()),
        "fillRequest" => {
            let b2 = b.clone();
            let url = page_url.to_string();
            tauri::async_runtime::spawn(async move {
                let p = b2.protector.clone();
                let u = url.clone();
                let logins = b2.db.run(move |db| vault::for_page(db.conn(), &*p, &u)).await.unwrap_or_default();
                let exact: Vec<_> = logins.iter().filter(|l| !l.same_site_only).collect();
                if exact.len() == 1 {
                    let _ = fill(&b2, id, exact[0].id).await;
                } else if !logins.is_empty() {
                    b2.emit("autofill:choose", serde_json::json!({ "tabId": id }));
                }
            });
        }
        "submit" => {
            let (Some(u), Some(p)) = (msg.u, msg.p) else { return };
            if p.is_empty() || p.len() > 4096 || u.len() > 1024 {
                return;
            }
            offer_save(b, id, page_url.to_string(), Zeroizing::new(u), Zeroizing::new(p));
        }
        _ => {
            let _ = msg.count;
        }
    }
}

/// Tells the UI (key icon in the omnibox) and the page (fill chip) that
/// logins exist for this page.
fn announce(b: &SharedBrowser, id: TabId, url: String) {
    let b2 = b.clone();
    tauri::async_runtime::spawn(async move {
        let p = b2.protector.clone();
        let u = url.clone();
        let logins = b2.db.run(move |db| vault::for_page(db.conn(), &*p, &u)).await.unwrap_or_default();
        let count = logins.len();
        b2.emit("autofill:available", serde_json::json!({ "tabId": id, "count": count }));
        if count == 0 {
            return;
        }
        let wv = b2.inner.lock().webview(id);
        if let Some(wv) = wv {
            let json = serde_json::json!({ "limbo": 1, "t": "hasLogins", "count": count }).to_string();
            webview2::with_core(&wv, move |_, core| unsafe {
                let _ = core.PostWebMessageAsJson(&HSTRING::from(json));
            });
        }
    });
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct SavePromptEvent {
    prompt_id: u64,
    tab_id: TabId,
    origin: String,
    username: String,
    update: bool,
}

fn offer_save(b: &SharedBrowser, id: TabId, url: String, username: Zeroizing<String>, password: Zeroizing<String>) {
    let (private, enabled) = {
        let inner = b.inner.lock();
        (inner.tab(id).map(|t| t.info.private).unwrap_or(true), inner.settings.offer_to_save_passwords)
    };
    if private || !enabled {
        return;
    }
    let b2 = b.clone();
    tauri::async_runtime::spawn(async move {
        let p = b2.protector.clone();
        let (u, pw, url2) = (username.clone(), password.clone(), url.clone());
        let decision = b2.db.run(move |db| vault::save_prompt(db.conn(), &*p, &url2, &u, &pw)).await;
        let update = match decision {
            Ok(SavePrompt::Save) => false,
            Ok(SavePrompt::Update { .. }) => true,
            _ => return,
        };
        let Some(origin) = limbo_core::omnibox::origin_of(&url) else { return };
        let b3 = b2.clone();
        let origin2 = origin.clone();
        let user_for_ui = username.to_string();
        let prompt_id = b2.pending.add(Box::new(move |answer| {
            let action = answer.get("action").and_then(|a| a.as_str()).unwrap_or("notNow").to_string();
            tauri::async_runtime::spawn(async move {
                match action.as_str() {
                    "save" => {
                        let p = b3.protector.clone();
                        let login = NewLogin::new(&origin2, &username, &password);
                        let r = b3
                            .db
                            .run(move |db| {
                                vault::save(
                                    db.conn(),
                                    &*p,
                                    &login,
                                    vault::LoginSource::Native,
                                    limbo_core::db::now_us(),
                                )
                            })
                            .await;
                        if r.is_ok() {
                            b3.toast("Password saved");
                        }
                    }
                    "never" => {
                        let o = origin2.clone();
                        let _ = b3.db.run(move |db| vault::never_save(db.conn(), &o)).await;
                    }
                    _ => {}
                }
            });
        }));
        b2.emit(
            "passwords:save-prompt",
            SavePromptEvent { prompt_id, tab_id: id, origin, username: user_for_ui, update },
        );
    });
}

/// Fills a saved login into the tab, after re-checking the page's origin.
pub async fn fill(b: &SharedBrowser, tab_id: TabId, login_id: i64) -> Result<(), String> {
    let wv = b.inner.lock().webview(tab_id).ok_or("no such tab")?;
    let current = webview2::with_core_result(&wv, |_, core| webview2::source(&core)).await.ok_or("tab is gone")?;
    let p = b.protector.clone();
    let url = current.clone();
    let (login, password) =
        b.db.run(move |db| {
            let logins = vault::for_page(db.conn(), &*p, &url)?;
            let login = logins
                .into_iter()
                .find(|l| l.id == login_id)
                .ok_or_else(|| limbo_core::Error::NotFound("login for this page".into()))?;
            let pw = vault::password(db.conn(), &*p, login_id)?;
            vault::touch_used(db.conn(), login_id, limbo_core::db::now_us())?;
            Ok((login, pw))
        })
        .await?;
    if url::Url::parse(&current).map(|u| u.scheme() == "http").unwrap_or(false) && !login.origin.starts_with("http:") {
        return Err("won't fill an https password into an http page".into());
    }
    let script = Zeroizing::new(fill_script(&login.username, &password));
    webview2::with_core(&wv, move |_, core| webview2::execute_script(&core, &script));
    Ok(())
}

fn fill_script(username: &str, password: &str) -> String {
    let u = serde_json::to_string(username).unwrap_or_default();
    let p = serde_json::to_string(password).unwrap_or_default();
    format!(
        r#"(function(u,p){{
  const vis=(el)=>{{if(!el||el.disabled||el.readOnly)return false;const r=el.getBoundingClientRect();return r.width>4&&r.height>4;}};
  const set=(el,v)=>{{const d=Object.getOwnPropertyDescriptor(HTMLInputElement.prototype,'value');d.set.call(el,v);
    el.dispatchEvent(new Event('input',{{bubbles:true}}));el.dispatchEvent(new Event('change',{{bubbles:true}}));}};
  const pw=[...document.querySelectorAll('input[type="password"]')].find(vis);
  let user=null;
  if(pw){{const scope=pw.form||document;const ins=[...scope.querySelectorAll('input')];const i=ins.indexOf(pw);
    for(let j=i-1;j>=0;j--){{const t=(ins[j].getAttribute('type')||'').toLowerCase();if(['text','email','tel',''].includes(t)&&vis(ins[j])){{user=ins[j];break;}}}}}}
  if(!user)user=[...document.querySelectorAll('input[autocomplete~="username"],input[type="email"]')].find(vis)||null;
  if(user&&u)set(user,u);
  if(pw)set(pw,p);
  (pw||user)&&(pw||user).focus();
}})({u},{p});"#
    )
}

/// Reveal/copy in Settings: Windows Hello (or the Windows password) first.
pub async fn reveal(b: &SharedBrowser, id: i64) -> Result<String, String> {
    let hwnd = b.window.hwnd().map_err(|e| e.to_string())?.0 as isize;
    let ok = tauri::async_runtime::spawn_blocking(move || verify_user(hwnd, "Limbo wants to show a saved password"))
        .await
        .map_err(|e| e.to_string())?;
    if !ok {
        return Err("verification cancelled".into());
    }
    let p = b.protector.clone();
    b.db.run(move |db| vault::password(db.conn(), &*p, id).map(|s| s.to_string())).await
}

pub async fn export(b: &SharedBrowser) -> Result<Option<String>, String> {
    let hwnd = b.window.hwnd().map_err(|e| e.to_string())?.0 as isize;
    let ok =
        tauri::async_runtime::spawn_blocking(move || verify_user(hwnd, "Limbo wants to export your saved passwords"))
            .await
            .map_err(|e| e.to_string())?;
    if !ok {
        return Err("verification cancelled".into());
    }
    let Some(path) = crate::dialog::save_file(b, "Export passwords", "Limbo Passwords.csv", &[("CSV", "*.csv")]).await
    else {
        return Ok(None);
    };
    let p = b.protector.clone();
    let text = b.db.run(move |db| vault::export_csv(db.conn(), &*p)).await?;
    std::fs::write(&path, text.as_bytes()).map_err(|e| e.to_string())?;
    Ok(Some(path.to_string_lossy().into_owned()))
}

pub async fn import_csv(b: &SharedBrowser) -> Result<Option<vault::ImportCounts>, String> {
    let Some(path) = crate::dialog::open_file(b, "Import passwords (CSV)", &[("CSV", "*.csv")]).await else {
        return Ok(None);
    };
    let text = Zeroizing::new(std::fs::read_to_string(&path).map_err(|e| e.to_string())?);
    let logins = vault::parse_csv(&text).map_err(|e| e.to_string())?;
    let p = b.protector.clone();
    let counts = b
        .db
        .run(move |db| vault::import(db.conn_mut(), &*p, &logins, vault::LoginSource::Csv, limbo_core::db::now_us()))
        .await?;
    Ok(Some(counts))
}
