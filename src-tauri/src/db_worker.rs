//! All SQLite work happens on one background thread, so the UI thread never
//! waits on disk. Callers send closures and await the result.

use std::sync::mpsc;
use std::thread;

use limbo_core::db::Db;
use tokio::sync::oneshot;

type Job = Box<dyn FnOnce(&mut Db) + Send>;

#[derive(Clone)]
pub struct DbHandle {
    tx: mpsc::Sender<Job>,
}

impl DbHandle {
    pub fn start(db: Db) -> DbHandle {
        let (tx, rx) = mpsc::channel::<Job>();
        thread::Builder::new()
            .name("limbo-db".into())
            .spawn(move || {
                let mut db = db;
                let mut idle_shrunk = false;
                loop {
                    match rx.recv_timeout(std::time::Duration::from_secs(30)) {
                        Ok(job) => {
                            job(&mut db);
                            idle_shrunk = false;
                        }
                        Err(mpsc::RecvTimeoutError::Timeout) => {
                            // Give SQLite's page cache back while nothing is happening.
                            if !idle_shrunk {
                                db.shrink_memory();
                                idle_shrunk = true;
                            }
                        }
                        Err(mpsc::RecvTimeoutError::Disconnected) => break,
                    }
                }
            })
            .expect("spawn db thread");
        DbHandle { tx }
    }

    /// Runs `f` on the database thread and returns its result.
    pub async fn call<T: Send + 'static>(&self, f: impl FnOnce(&mut Db) -> T + Send + 'static) -> Result<T, String> {
        let (otx, orx) = oneshot::channel();
        self.tx
            .send(Box::new(move |db| {
                let _ = otx.send(f(db));
            }))
            .map_err(|_| "database thread stopped".to_string())?;
        orx.await.map_err(|_| "database job dropped".to_string())
    }

    /// Like [`DbHandle::call`] for fallible jobs, flattening errors to strings for IPC.
    pub async fn run<T: Send + 'static>(
        &self,
        f: impl FnOnce(&mut Db) -> limbo_core::Result<T> + Send + 'static,
    ) -> Result<T, String> {
        self.call(f).await?.map_err(|e| e.to_string())
    }

    /// Blocking variant for non-async contexts (worker threads only, never the UI thread).
    pub fn call_blocking<T: Send + 'static>(&self, f: impl FnOnce(&mut Db) -> T + Send + 'static) -> Option<T> {
        let (otx, orx) = mpsc::channel();
        self.tx
            .send(Box::new(move |db| {
                let _ = otx.send(f(db));
            }))
            .ok()?;
        orx.recv().ok()
    }

    /// Fire and forget (history recording, favicon caching).
    pub fn spawn(&self, f: impl FnOnce(&mut Db) + Send + 'static) {
        let _ = self.tx.send(Box::new(f));
    }
}
