//! `LimboPortable.exe`, the launcher at the root of the PortableApps.com
//! Format package:
//!
//! ```text
//! LimboPortable\
//!   LimboPortable.exe      this program
//!   App\Limbo\Limbo.exe    the browser
//!   App\AppInfo\           name, version and icons for the PortableApps.com Platform
//!   Data\                  profile, passwords, history, extensions (created on first run)
//! ```
//!
//! It starts `Limbo.exe` with `LIMBO_DATA_DIR` pointing at `Data\`, passes the
//! command line through (links opened from the Platform), and exits at once so
//! no extra process stays in memory. Limbo does the rest: it writes nothing
//! outside `Data\` and nothing to the registry in portable mode.

#![cfg_attr(windows, windows_subsystem = "windows")]

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::Command;

/// Must match `DATA_DIR_ENV` in `src-tauri/src/paths.rs`.
const DATA_DIR_ENV: &str = "LIMBO_DATA_DIR";

struct Layout {
    exe: PathBuf,
    workdir: PathBuf,
    data: PathBuf,
}

fn layout(root: &Path) -> Layout {
    let workdir = root.join("App").join("Limbo");
    Layout { exe: workdir.join("Limbo.exe"), workdir, data: root.join("Data") }
}

fn launch(root: &Path, args: impl IntoIterator<Item = OsString>) -> Result<(), String> {
    let l = layout(root);
    if !l.exe.is_file() {
        return Err(format!("{} is missing. Reinstall Limbo Portable.", l.exe.display()));
    }
    std::fs::create_dir_all(&l.data).map_err(|e| format!("Can't create {}: {e}", l.data.display()))?;
    Command::new(&l.exe)
        .args(args)
        .current_dir(&l.workdir)
        .env(DATA_DIR_ENV, &l.data)
        .spawn()
        .map(drop)
        .map_err(|e| format!("Can't start Limbo: {e}"))
}

fn main() {
    let root = std::env::current_exe().ok().and_then(|p| p.parent().map(Path::to_path_buf));
    let result = match root {
        Some(root) => launch(&root, std::env::args_os().skip(1)),
        None => Err("Can't find where Limbo Portable is.".into()),
    };
    if let Err(message) = result {
        show_error(&message);
        std::process::exit(1);
    }
}

#[cfg(windows)]
fn show_error(message: &str) {
    use windows_sys::Win32::UI::WindowsAndMessaging::{MB_ICONERROR, MB_OK, MessageBoxW};
    let wide = |s: &str| s.encode_utf16().chain(Some(0)).collect::<Vec<u16>>();
    let (text, caption) = (wide(message), wide("Limbo Portable"));
    unsafe {
        MessageBoxW(std::ptr::null_mut(), text.as_ptr(), caption.as_ptr(), MB_OK | MB_ICONERROR);
    }
}

#[cfg(not(windows))]
fn show_error(message: &str) {
    eprintln!("{message}");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn package_layout() {
        let root = Path::new("E:/PortableApps/LimboPortable");
        let l = layout(root);
        assert_eq!(l.exe, root.join("App").join("Limbo").join("Limbo.exe"));
        assert_eq!(l.data, root.join("Data"));
    }

    #[test]
    fn missing_app_is_reported() {
        let root = std::env::temp_dir().join(format!("limbo-launcher-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        let err = launch(&root, Vec::<OsString>::new()).unwrap_err();
        assert!(err.contains("Limbo.exe is missing"), "{err}");
        assert!(!root.join("Data").exists(), "no Data folder without the app");
    }

    #[cfg(unix)]
    #[test]
    fn starts_the_app_with_the_data_folder() {
        use std::os::unix::fs::PermissionsExt;
        let root = std::env::temp_dir().join(format!("limbo-launcher-run-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let l = layout(&root);
        std::fs::create_dir_all(&l.workdir).unwrap();
        // A stand-in "Limbo.exe" that records its environment and arguments.
        let out = root.join("out.txt");
        std::fs::write(&l.exe, format!("#!/bin/sh\necho \"$LIMBO_DATA_DIR|$1|$(pwd)\" > '{}'\n", out.display()))
            .unwrap();
        std::fs::set_permissions(&l.exe, std::fs::Permissions::from_mode(0o755)).unwrap();
        launch(&root, vec![OsString::from("https://example.com/")]).unwrap();
        for _ in 0..50 {
            if out.exists() {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        let got = std::fs::read_to_string(&out).unwrap();
        let workdir = l.workdir.canonicalize().unwrap();
        assert_eq!(got.trim(), format!("{}|https://example.com/|{}", l.data.display(), workdir.display()));
        assert!(l.data.is_dir());
    }
}
