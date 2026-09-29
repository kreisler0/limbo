//! Opens Firefox databases without ever touching the profile: each database is
//! copied (with its `-wal`/`-shm` files) to a private temp folder and opened
//! there, so a running Firefox and its uncheckpointed WAL are both handled.
//! (`?immutable=1` on the original would ignore the WAL and miss recent data.)

use std::path::{Path, PathBuf};

use rusqlite::{Connection, OpenFlags};

use crate::error::{Error, Result};

/// A temp folder deleted on drop.
pub struct TempDir {
    path: PathBuf,
}

impl TempDir {
    pub fn new(prefix: &str) -> Result<TempDir> {
        let path = std::env::temp_dir().join(format!("{prefix}-{}", uuid::Uuid::new_v4().simple()));
        std::fs::create_dir_all(&path)?;
        Ok(TempDir { path })
    }

    pub fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.path);
    }
}

/// A database copied out of a profile. Keep it alive while using `conn`.
pub struct DbSnapshot {
    pub conn: Connection,
    _dir: TempDir,
}

/// Copies `profile/<name>` (+ `-wal`, `-shm`) and opens the copy.
pub fn open(profile: &Path, name: &str) -> Result<DbSnapshot> {
    let src = profile.join(name);
    if !src.is_file() {
        return Err(Error::NotFound(name.to_string()));
    }
    let dir = TempDir::new("limbo-import")?;
    let dst = dir.path().join(name);
    std::fs::copy(&src, &dst)?;
    for suffix in ["-wal", "-shm"] {
        let side = profile.join(format!("{name}{suffix}"));
        if side.is_file() {
            // A missing or unreadable -shm is fine: SQLite rebuilds it from the WAL.
            let _ = std::fs::copy(&side, dir.path().join(format!("{name}{suffix}")));
        }
    }
    let conn = Connection::open_with_flags(&dst, OpenFlags::SQLITE_OPEN_READ_WRITE | OpenFlags::SQLITE_OPEN_NO_MUTEX)?;
    conn.execute_batch("PRAGMA query_only = ON;")?;
    Ok(DbSnapshot { conn, _dir: dir })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn copies_and_replays_wal() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("places.sqlite");
        let writer = Connection::open(&path).unwrap();
        writer.execute_batch("PRAGMA journal_mode=WAL; PRAGMA wal_autocheckpoint=0; CREATE TABLE t(x); INSERT INTO t VALUES (1), (2);").unwrap();
        // The writer is still open (like a running Firefox): rows live only in the WAL.
        assert!(dir.path().join("places.sqlite-wal").exists());
        let snap = open(dir.path(), "places.sqlite").unwrap();
        let n: i64 = snap.conn.query_row("SELECT COUNT(*) FROM t", [], |r| r.get(0)).unwrap();
        assert_eq!(n, 2);
        assert!(snap.conn.execute("INSERT INTO t VALUES (3)", []).is_err(), "read only");
        drop(writer);
        assert!(matches!(open(dir.path(), "missing.sqlite"), Err(Error::NotFound(_))));
    }
}
