//! Limbo core: everything that doesn't need WebView2 or Win32, so it can be
//! unit-tested on any OS.

pub mod bookmarks;
pub mod csv;
pub mod db;
pub mod downloads;
pub mod error;
pub mod extensions;
pub mod favicons;
pub mod frecency;
pub mod history;
pub mod import;
pub mod lifecycle;
pub mod memory;
pub mod omnibox;
pub mod sessions;
pub mod settings;
pub mod sites;
pub mod suggest;
pub mod vault;

pub use error::{Error, Result};
