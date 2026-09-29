//! Where Limbo keeps its files: `%LOCALAPPDATA%\Limbo\...`.

use std::path::PathBuf;

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
}

impl Paths {
    pub fn new() -> std::io::Result<Paths> {
        let base = std::env::var_os("LOCALAPPDATA")
            .map(PathBuf::from)
            .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".local").join("share")))
            .unwrap_or_else(std::env::temp_dir);
        let root = base.join("Limbo");
        let paths = Paths {
            profile: root.join("Profile"),
            db: root.join("browser.db"),
            extensions: root.join("Extensions"),
            snapshots: root.join("Snapshots"),
            logs: root.join("Logs"),
            staging: root.join("Staging"),
            root,
        };
        for dir in [&paths.root, &paths.profile, &paths.extensions, &paths.snapshots, &paths.logs, &paths.staging] {
            std::fs::create_dir_all(dir)?;
        }
        Ok(paths)
    }

    pub fn snapshot(&self, tab_id: u64) -> PathBuf {
        self.snapshots.join(format!("{tab_id}.jpg"))
    }
}
