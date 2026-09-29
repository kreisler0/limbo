//! Native file dialogs (IFileOpenDialog / IFileSaveDialog) without a plugin.
//! They run on the UI thread via `run_on_main_thread`, never inside a WebView2 callback.

use std::path::PathBuf;

use tokio::sync::oneshot;
use windows::Win32::Foundation::HWND;
use windows::Win32::System::Com::{CLSCTX_INPROC_SERVER, CoCreateInstance, CoTaskMemFree};
use windows::Win32::UI::Shell::Common::COMDLG_FILTERSPEC;
use windows::Win32::UI::Shell::{
    FOS_FORCEFILESYSTEM, FOS_OVERWRITEPROMPT, FOS_PICKFOLDERS, FileOpenDialog, FileSaveDialog, IFileDialog,
    IFileOpenDialog, IFileSaveDialog, SIGDN_FILESYSPATH,
};
use windows_core::{HSTRING, Interface, PCWSTR};

use crate::state::Browser;

#[derive(Clone, Copy)]
enum Kind {
    Open,
    Folder,
    Save,
}

unsafe fn run(hwnd: HWND, kind: Kind, title: &str, default_name: &str, filters: &[(&str, &str)]) -> Option<PathBuf> {
    unsafe {
        let dialog: IFileDialog = match kind {
            Kind::Save => {
                CoCreateInstance::<_, IFileSaveDialog>(&FileSaveDialog, None, CLSCTX_INPROC_SERVER).ok()?.cast().ok()?
            }
            _ => {
                CoCreateInstance::<_, IFileOpenDialog>(&FileOpenDialog, None, CLSCTX_INPROC_SERVER).ok()?.cast().ok()?
            }
        };
        let _ = dialog.SetTitle(&HSTRING::from(title));
        let mut opts = dialog.GetOptions().ok()? | FOS_FORCEFILESYSTEM;
        match kind {
            Kind::Folder => opts |= FOS_PICKFOLDERS,
            Kind::Save => opts |= FOS_OVERWRITEPROMPT,
            Kind::Open => {}
        }
        let _ = dialog.SetOptions(opts);
        if !default_name.is_empty() {
            let _ = dialog.SetFileName(&HSTRING::from(default_name));
        }
        // Keep the strings alive while the dialog uses them.
        let owned: Vec<(HSTRING, HSTRING)> =
            filters.iter().map(|(n, p)| (HSTRING::from(*n), HSTRING::from(*p))).collect();
        let specs: Vec<COMDLG_FILTERSPEC> = owned
            .iter()
            .map(|(n, p)| COMDLG_FILTERSPEC { pszName: PCWSTR(n.as_ptr()), pszSpec: PCWSTR(p.as_ptr()) })
            .collect();
        if !specs.is_empty() {
            let _ = dialog.SetFileTypes(&specs);
        }
        dialog.Show(Some(hwnd)).ok()?;
        let item = dialog.GetResult().ok()?;
        let name = item.GetDisplayName(SIGDN_FILESYSPATH).ok()?;
        let path = name.to_string().ok();
        CoTaskMemFree(Some(name.0 as *const _));
        path.map(PathBuf::from)
    }
}

async fn show(b: &Browser, kind: Kind, title: &str, default_name: &str, filters: &[(&str, &str)]) -> Option<PathBuf> {
    let hwnd = b.window.hwnd().ok()?.0 as isize;
    let (tx, rx) = oneshot::channel();
    let title = title.to_string();
    let default_name = default_name.to_string();
    let filters: Vec<(String, String)> = filters.iter().map(|(a, c)| (a.to_string(), c.to_string())).collect();
    b.app
        .run_on_main_thread(move || {
            let f: Vec<(&str, &str)> = filters.iter().map(|(a, c)| (a.as_str(), c.as_str())).collect();
            let r = unsafe { run(HWND(hwnd as *mut _), kind, &title, &default_name, &f) };
            let _ = tx.send(r);
        })
        .ok()?;
    rx.await.ok().flatten()
}

pub async fn open_file(b: &Browser, title: &str, filters: &[(&str, &str)]) -> Option<PathBuf> {
    show(b, Kind::Open, title, "", filters).await
}

pub async fn pick_folder(b: &Browser, title: &str) -> Option<PathBuf> {
    show(b, Kind::Folder, title, "", &[]).await
}

pub async fn save_file(b: &Browser, title: &str, default_name: &str, filters: &[(&str, &str)]) -> Option<PathBuf> {
    show(b, Kind::Save, title, default_name, filters).await
}
