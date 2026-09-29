//! Registers Limbo as a candidate default browser (per user, no admin) and
//! opens Windows Settings, where the user makes the choice (Windows doesn't let
//! apps set it themselves).

use windows::Win32::System::Registry::{
    HKEY, HKEY_CURRENT_USER, KEY_READ, KEY_WRITE, REG_OPTION_NON_VOLATILE, REG_SZ, RRF_RT_REG_SZ, RegCloseKey,
    RegCreateKeyExW, RegGetValueW, RegSetValueExW,
};
use windows::Win32::UI::Shell::{SHCNE_ASSOCCHANGED, SHCNF_IDLIST, SHChangeNotify, ShellExecuteW};
use windows::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;
use windows_core::{HSTRING, PCWSTR};

const PROG_ID: &str = "LimboHTML";
const CLIENT: &str = r"Software\Clients\StartMenuInternet\Limbo";

fn set(path: &str, name: Option<&str>, value: &str) -> windows_core::Result<()> {
    unsafe {
        let mut key = HKEY::default();
        RegCreateKeyExW(
            HKEY_CURRENT_USER,
            &HSTRING::from(path),
            None,
            None,
            REG_OPTION_NON_VOLATILE,
            KEY_WRITE,
            None,
            &mut key,
            None,
        )
        .ok()?;
        let wide: Vec<u16> = value.encode_utf16().chain(std::iter::once(0)).collect();
        let bytes = std::slice::from_raw_parts(wide.as_ptr() as *const u8, wide.len() * 2);
        let name = name.map(HSTRING::from);
        let r = RegSetValueExW(
            key,
            name.as_ref().map(|n| PCWSTR(n.as_ptr())).unwrap_or(PCWSTR::null()),
            None,
            REG_SZ,
            Some(bytes),
        )
        .ok();
        let _ = RegCloseKey(key);
        r
    }
}

pub fn register() -> windows_core::Result<()> {
    let exe = std::env::current_exe().map(|p| p.to_string_lossy().into_owned()).unwrap_or_default();
    let open = format!("\"{exe}\" \"%1\"");
    let icon = format!("{exe},0");
    let classes = format!(r"Software\Classes\{PROG_ID}");
    set(&classes, None, "Limbo HTML Document")?;
    set(&format!(r"{classes}\DefaultIcon"), None, &icon)?;
    set(&format!(r"{classes}\shell\open\command"), None, &open)?;
    set(CLIENT, None, "Limbo")?;
    set(&format!(r"{CLIENT}\DefaultIcon"), None, &icon)?;
    set(&format!(r"{CLIENT}\shell\open\command"), None, &format!("\"{exe}\""))?;
    let caps = format!(r"{CLIENT}\Capabilities");
    set(&caps, Some("ApplicationName"), "Limbo")?;
    set(&caps, Some("ApplicationDescription"), "A lightweight browser")?;
    set(&caps, Some("ApplicationIcon"), &icon)?;
    for scheme in ["http", "https"] {
        set(&format!(r"{caps}\URLAssociations"), Some(scheme), PROG_ID)?;
    }
    for ext in [".html", ".htm", ".xhtml", ".svg", ".pdf"] {
        set(&format!(r"{caps}\FileAssociations"), Some(ext), PROG_ID)?;
    }
    set(r"Software\RegisteredApplications", Some("Limbo"), &caps)?;
    unsafe { SHChangeNotify(SHCNE_ASSOCCHANGED, SHCNF_IDLIST, None, None) };
    Ok(())
}

pub fn is_default() -> bool {
    let mut buf = [0u16; 64];
    let mut size = (buf.len() * 2) as u32;
    let r = unsafe {
        RegGetValueW(
            HKEY_CURRENT_USER,
            &HSTRING::from(r"Software\Microsoft\Windows\Shell\Associations\UrlAssociations\https\UserChoice"),
            &HSTRING::from("ProgId"),
            RRF_RT_REG_SZ,
            None,
            Some(buf.as_mut_ptr() as *mut _),
            Some(&mut size),
        )
    };
    let _ = KEY_READ;
    r.is_ok() && String::from_utf16_lossy(&buf[..(size as usize / 2).saturating_sub(1)]) == PROG_ID
}

/// Registers, then opens Settings > Default apps at Limbo's page.
pub fn open_settings() {
    if let Err(e) = register() {
        log::warn!("default browser registration failed: {e}");
    }
    unsafe {
        ShellExecuteW(
            None,
            &HSTRING::from("open"),
            &HSTRING::from("ms-settings:defaultapps?registeredAppUser=Limbo"),
            None,
            None,
            SW_SHOWNORMAL,
        );
    }
}
