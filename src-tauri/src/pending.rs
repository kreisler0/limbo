//! Requests waiting for the user (permission prompts, context menus, password
//! save prompts, extension install reviews). The UI answers with an id.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};

use parking_lot::Mutex;

pub type Responder = Box<dyn FnOnce(serde_json::Value) + Send>;

#[derive(Default)]
pub struct Pending {
    next: AtomicU64,
    map: Mutex<HashMap<u64, Responder>>,
}

impl Pending {
    pub fn add(&self, responder: Responder) -> u64 {
        let id = self.next.fetch_add(1, Ordering::Relaxed) + 1;
        self.map.lock().insert(id, responder);
        id
    }

    pub fn take(&self, id: u64) -> Option<Responder> {
        self.map.lock().remove(&id)
    }

    /// Answers a request (the responder decides what the value means).
    pub fn respond(&self, id: u64, value: serde_json::Value) -> bool {
        match self.take(id) {
            Some(r) => {
                r(value);
                true
            }
            None => false,
        }
    }
}

/// Wraps a value that must only be touched on the thread that created it
/// (WebView2 COM objects live in the UI thread's apartment). It can be moved
/// through `Send` channels, but access from another thread returns `None`.
pub struct ThreadBound<T> {
    value: Option<T>,
    owner: std::thread::ThreadId,
}

// SAFETY: the value is only ever accessed (and dropped) on `owner`; see `get` and `Drop`.
unsafe impl<T> Send for ThreadBound<T> {}
unsafe impl<T> Sync for ThreadBound<T> {}

impl<T> ThreadBound<T> {
    pub fn new(value: T) -> ThreadBound<T> {
        ThreadBound { value: Some(value), owner: std::thread::current().id() }
    }

    pub fn get(&self) -> Option<&T> {
        if std::thread::current().id() == self.owner { self.value.as_ref() } else { None }
    }

    pub fn into_inner(mut self) -> Option<T> {
        if std::thread::current().id() == self.owner { self.value.take() } else { None }
    }
}

impl<T> Drop for ThreadBound<T> {
    fn drop(&mut self) {
        if let Some(v) = self.value.take() {
            if std::thread::current().id() == self.owner {
                drop(v);
            } else {
                // Releasing a COM pointer off its apartment is unsafe; leak it instead.
                log::warn!("ThreadBound value dropped on the wrong thread; leaking it");
                std::mem::forget(v);
            }
        }
    }
}
