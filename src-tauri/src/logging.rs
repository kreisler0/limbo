//! A tiny file logger: `Logs\limbo.log`, rotated at 1 MB, 5 files kept.
//! No telemetry; URLs are never logged at info level or above.

use std::fs::{File, OpenOptions};
use std::io::Write;
use std::path::PathBuf;

use log::{Level, LevelFilter, Log, Metadata, Record};
use parking_lot::Mutex;

const MAX_BYTES: u64 = 1024 * 1024;
const KEEP: usize = 5;

struct FileLogger {
    dir: PathBuf,
    file: Mutex<Option<(File, u64)>>,
    level: Level,
}

impl FileLogger {
    fn path(&self, n: usize) -> PathBuf {
        if n == 0 { self.dir.join("limbo.log") } else { self.dir.join(format!("limbo.{n}.log")) }
    }

    fn rotate(&self) {
        for n in (0..KEEP - 1).rev() {
            let _ = std::fs::rename(self.path(n), self.path(n + 1));
        }
    }

    fn open(&self) -> Option<(File, u64)> {
        let f = OpenOptions::new().create(true).append(true).open(self.path(0)).ok()?;
        let len = f.metadata().map(|m| m.len()).unwrap_or(0);
        Some((f, len))
    }
}

impl Log for FileLogger {
    fn enabled(&self, m: &Metadata<'_>) -> bool {
        m.level() <= self.level && (m.target().starts_with("limbo") || m.level() <= Level::Warn)
    }

    fn log(&self, r: &Record<'_>) {
        if !self.enabled(r.metadata()) {
            return;
        }
        let secs = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
        let line = format!("{secs} {:<5} {}: {}\n", r.level(), r.target(), r.args());
        let mut guard = self.file.lock();
        if guard.is_none() {
            *guard = self.open();
        }
        if let Some((_, len)) = guard.as_mut()
            && *len + line.len() as u64 > MAX_BYTES
        {
            *guard = None;
            self.rotate();
            *guard = self.open();
        }
        if let Some((f, len)) = guard.as_mut() {
            let _ = f.write_all(line.as_bytes());
            *len += line.len() as u64;
        }
    }

    fn flush(&self) {
        if let Some((f, _)) = self.file.lock().as_mut() {
            let _ = f.flush();
        }
    }
}

pub fn init(dir: PathBuf) {
    let level = if cfg!(debug_assertions) { Level::Debug } else { Level::Info };
    let logger = FileLogger { dir, file: Mutex::new(None), level };
    if log::set_boxed_logger(Box::new(logger)).is_ok() {
        log::set_max_level(if cfg!(debug_assertions) { LevelFilter::Debug } else { LevelFilter::Info });
    }
}
