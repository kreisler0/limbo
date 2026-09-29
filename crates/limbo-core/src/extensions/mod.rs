//! Chrome (MV3) extensions: store URLs, CRX3 verification, manifests, updates,
//! and safe unpacking into immutable per-version folders.

pub mod addon_map;
pub mod crx;
pub mod manifest;
pub mod registry;
pub mod store;
pub mod update;

use std::io::Read;
use std::path::{Path, PathBuf};

use crate::error::{Error, Result};

/// 32 letters a–p.
pub fn is_valid_id(id: &str) -> bool {
    id.len() == 32 && id.bytes().all(|b| (b'a'..=b'p').contains(&b))
}

/// Largest archive we'll unpack (uncompressed), to stop zip bombs.
const MAX_UNPACKED_BYTES: u64 = 512 * 1024 * 1024;

/// Unpacks a ZIP (CRX payload, .zip, .xpi) into `dest`, which must not exist.
/// Entries escaping the folder are rejected. The folder is never modified
/// afterwards: WebView2 uninstalls an extension whose folder changes.
pub fn unpack(archive: &[u8], dest: &Path) -> Result<()> {
    if dest.exists() {
        return Err(Error::Format(format!("{} already exists", dest.display())));
    }
    let staging = dest.with_extension("partial");
    let _ = std::fs::remove_dir_all(&staging);
    std::fs::create_dir_all(&staging)?;
    let result = (|| -> Result<()> {
        let mut zip = zip::ZipArchive::new(std::io::Cursor::new(archive))?;
        let mut total = 0u64;
        for i in 0..zip.len() {
            let mut entry = zip.by_index(i)?;
            let Some(rel) = entry.enclosed_name() else {
                return Err(Error::Format(format!("unsafe path in archive: {}", entry.name())));
            };
            // Chrome ignores the signature folder of re-packed CRXs; so do we.
            if rel.starts_with("_metadata") && rel.ends_with("computed_hashes.json") {
                continue;
            }
            let out: PathBuf = staging.join(rel);
            if entry.is_dir() {
                std::fs::create_dir_all(&out)?;
                continue;
            }
            if let Some(parent) = out.parent() {
                std::fs::create_dir_all(parent)?;
            }
            let mut buf = Vec::with_capacity(entry.size().min(16 * 1024 * 1024) as usize);
            (&mut entry).take(MAX_UNPACKED_BYTES - total + 1).read_to_end(&mut buf)?;
            total += buf.len() as u64;
            if total > MAX_UNPACKED_BYTES {
                return Err(Error::Format("archive is too large".into()));
            }
            std::fs::write(&out, &buf)?;
        }
        if !staging.join("manifest.json").is_file() {
            return Err(Error::Format("the package has no manifest.json".into()));
        }
        Ok(())
    })();
    match result {
        Ok(()) => {
            if let Some(parent) = dest.parent() {
                std::fs::create_dir_all(parent)?;
            }
            std::fs::rename(&staging, dest)?;
            Ok(())
        }
        Err(e) => {
            let _ = std::fs::remove_dir_all(&staging);
            Err(e)
        }
    }
}

/// `Extensions\<id>\<version>` under the app data folder.
pub fn install_dir(root: &Path, id: &str, version: &str) -> PathBuf {
    let safe_version: String =
        version.chars().map(|c| if c.is_ascii_alphanumeric() || c == '.' || c == '-' { c } else { '_' }).collect();
    root.join(id).join(safe_version)
}

/// Removes version folders of `id` other than `keep` (after a successful update).
pub fn remove_old_versions(root: &Path, id: &str, keep: &Path) -> Result<usize> {
    let mut n = 0;
    let dir = root.join(id);
    let Ok(entries) = std::fs::read_dir(&dir) else { return Ok(0) };
    for e in entries.flatten() {
        let p = e.path();
        if p != keep && p.is_dir() {
            std::fs::remove_dir_all(&p)?;
            n += 1;
        }
    }
    Ok(n)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids() {
        assert!(is_valid_id("ddkjiahejlhfcafbddmgiahcphecmpfh"));
        assert!(!is_valid_id("ddkjiahejlhfcafbddmgiahcphecmpfz"));
        assert!(!is_valid_id("short"));
    }

    #[test]
    fn unpack_and_versions() {
        let dir = tempfile::tempdir().unwrap();
        let zip = crx::testutil::zip_with(&[("manifest.json", "{}"), ("js/bg.js", "x"), ("_metadata/computed_hashes.json", "{}")]);
        let v1 = install_dir(dir.path(), "abc", "1.0");
        unpack(&zip, &v1).unwrap();
        assert!(v1.join("js/bg.js").is_file());
        assert!(!v1.join("_metadata/computed_hashes.json").exists());
        assert!(unpack(&zip, &v1).is_err(), "never overwrite an installed folder");
        let v2 = install_dir(dir.path(), "abc", "2.0 beta/..");
        assert!(v2.ends_with("2.0_beta_.."), "{v2:?}");
        unpack(&zip, &v2).unwrap();
        assert_eq!(remove_old_versions(dir.path(), "abc", &v2).unwrap(), 1);
        assert!(!v1.exists());

        let no_manifest = crx::testutil::zip_with(&[("readme.txt", "hi")]);
        let v3 = install_dir(dir.path(), "abc", "3");
        assert!(unpack(&no_manifest, &v3).is_err());
        assert!(!v3.exists());
        assert!(!v3.with_extension("partial").exists());
    }

    #[test]
    fn rejects_path_traversal() {
        let dir = tempfile::tempdir().unwrap();
        let evil = crx::testutil::zip_with(&[("manifest.json", "{}"), ("../escape.txt", "x")]);
        let dest = dir.path().join("x").join("1");
        assert!(unpack(&evil, &dest).is_err());
        assert!(!dir.path().join("x").join("escape.txt").exists());
    }
}
