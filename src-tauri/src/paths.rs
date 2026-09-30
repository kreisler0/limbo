//! Where Limbo keeps its files.
//!
//! Installed: `%LOCALAPPDATA%\Limbo\...`.
//! Portable (PortableApps.com Format): the launcher sets `LIMBO_DATA_DIR` to
//! the package's `Data\` folder, and everything lives there instead; a `Data`
//! folder next to `Limbo.exe` does the same for a plain portable copy. A
//! portable Limbo writes nothing to `%LOCALAPPDATA%` or the registry.

use std::path::{Path, PathBuf};

/// Set by `LimboPortable.exe` (packaging/portableapps/launcher).
pub const DATA_DIR_ENV: &str = "LIMBO_DATA_DIR";
/// In portable mode, the absolute folder Limbo last ran from. When the
/// package moves (another drive letter on a USB stick), absolute paths saved
/// by the engine (extension folders) must be fixed up.
const LAST_ROOT_FILE: &str = "last-root.txt";

#[derive(Debug, Clone)]
pub struct Paths {
    pub root: PathBuf,
    /// The one WebView2 user data folder shared by every webview.
    pub profile: PathBuf,
    pub db: PathBuf,
    /// Unpacked extensions: `Extensions\<id>\<version>\` (never modified after install).
    pub extensions: PathBuf,
    /// Tab snapshots (JPEG) for discarded tabs and overlays.
    pub snapshots: PathBuf,
    pub logs: PathBuf,
    /// Downloads staged for extension installs.
    pub staging: PathBuf,
    /// Running from a portable package (no registry, no %LOCALAPPDATA%).
    pub portable: bool,
    /// Portable only: where the data folder was on the previous run, if it moved.
    pub moved_from: Option<PathBuf>,
}

/// Decides where Limbo's data goes. Pure, so it can be tested anywhere.
pub fn resolve_root(
    env_data_dir: Option<PathBuf>,
    exe_dir: Option<&Path>,
    local_app_data: Option<PathBuf>,
) -> (PathBuf, bool) {
    if let Some(dir) = env_data_dir.filter(|d| d.is_absolute()) {
        return (dir, true);
    }
    if let Some(dir) = exe_dir.map(|d| d.join("Data")).filter(|d| d.is_dir()) {
        return (dir, true);
    }
    let base = local_app_data.unwrap_or_else(std::env::temp_dir);
    (base.join("Limbo"), false)
}

impl Paths {
    pub fn new() -> std::io::Result<Paths> {
        let exe_dir = std::env::current_exe().ok().and_then(|p| p.parent().map(Path::to_path_buf));
        let local = std::env::var_os("LOCALAPPDATA")
            .map(PathBuf::from)
            .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".local").join("share")));
        let (root, portable) =
            resolve_root(std::env::var_os(DATA_DIR_ENV).map(PathBuf::from), exe_dir.as_deref(), local);
        Paths::at(root, portable)
    }

    pub fn at(root: PathBuf, portable: bool) -> std::io::Result<Paths> {
        let mut paths = Paths {
            profile: root.join("Profile"),
            db: root.join("browser.db"),
            extensions: root.join("Extensions"),
            snapshots: root.join("Snapshots"),
            logs: root.join("Logs"),
            staging: root.join("Staging"),
            root,
            portable,
            moved_from: None,
        };
        for dir in [&paths.root, &paths.profile, &paths.extensions, &paths.snapshots, &paths.logs, &paths.staging] {
            std::fs::create_dir_all(dir)?;
        }
        if portable {
            let marker = paths.root.join(LAST_ROOT_FILE);
            let previous = std::fs::read_to_string(&marker).ok().map(|s| PathBuf::from(s.trim()));
            if let Some(prev) = previous.filter(|p| !p.as_os_str().is_empty() && *p != paths.root) {
                paths.moved_from = Some(prev);
            }
            let _ = std::fs::write(&marker, paths.root.to_string_lossy().as_bytes());
        }
        Ok(paths)
    }

    pub fn snapshot(&self, tab_id: u64) -> PathBuf {
        self.snapshots.join(format!("{tab_id}.jpg"))
    }

    /// Maps a path saved under an old root onto the current one
    /// (`E:\PortableApps\LimboPortable\Data\Extensions\x` → `F:\…\Extensions\x`).
    pub fn rebase(&self, path: &Path, old_root: &Path) -> Option<PathBuf> {
        path.strip_prefix(old_root).ok().map(|rel| self.root.join(rel))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("limbo-paths-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn installed_by_default() {
        let exe = temp("installed");
        let (root, portable) = resolve_root(None, Some(&exe), Some(PathBuf::from("C:\\Users\\a\\AppData\\Local")));
        assert!(!portable);
        assert!(root.ends_with("Limbo"));
    }

    #[test]
    fn launcher_env_wins() {
        let exe = temp("env");
        let data = exe.join("PortableData");
        let (root, portable) = resolve_root(Some(data.clone()), Some(&exe), None);
        assert!(portable);
        assert_eq!(root, data);
        // A relative value is ignored (it would depend on the working directory).
        let (_, portable) = resolve_root(Some(PathBuf::from("Data")), Some(&exe), None);
        assert!(!portable);
    }

    #[test]
    fn data_folder_next_to_exe() {
        let exe = temp("datadir");
        std::fs::create_dir_all(exe.join("Data")).unwrap();
        let (root, portable) = resolve_root(None, Some(&exe), None);
        assert!(portable);
        assert_eq!(root, exe.join("Data"));
    }

    #[test]
    fn detects_a_moved_package() {
        let a = temp("move-a");
        let b = temp("move-b");
        let first = Paths::at(a.clone(), true).unwrap();
        assert!(first.moved_from.is_none());
        // Same data, now seen at another place (drive letter changed).
        std::fs::copy(a.join(LAST_ROOT_FILE), b.join(LAST_ROOT_FILE)).unwrap();
        let moved = Paths::at(b.clone(), true).unwrap();
        assert_eq!(moved.moved_from.as_deref(), Some(a.as_path()));
        assert_eq!(
            moved.rebase(&a.join("Extensions").join("x").join("1.0"), &a),
            Some(b.join("Extensions").join("x").join("1.0"))
        );
        // The next run from the same place is not a move.
        assert!(Paths::at(b, true).unwrap().moved_from.is_none());
    }
}
