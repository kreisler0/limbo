//! Saved logins from `logins.json`, decrypted with the keys from `key4.db`.

use std::path::Path;

use serde::Deserialize;
use zeroize::Zeroizing;

use super::key4::{self, NssKeys, PbeKind};
use super::snapshot;
use crate::error::{Error, Result};
use crate::vault::{NewLogin, normalize_origin};

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct LoginsFile {
    #[serde(default)]
    logins: Vec<RawLogin>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawLogin {
    hostname: String,
    http_realm: Option<String>,
    form_submit_url: Option<String>,
    username_field: Option<String>,
    password_field: Option<String>,
    encrypted_username: String,
    encrypted_password: String,
    guid: Option<String>,
    time_created: Option<i64>,
    time_last_used: Option<i64>,
    time_password_changed: Option<i64>,
    times_used: Option<i64>,
}

pub struct DecryptedLogins {
    pub logins: Vec<NewLogin>,
    /// Entries that failed to decrypt (corrupted, or a key that no longer exists).
    pub failed: usize,
    /// Firefox-internal entries skipped on purpose (e.g. `chrome://FirefoxAccounts`).
    pub skipped: usize,
    /// Which PBE scheme protected the key database (logged).
    pub scheme: PbeKind,
}

impl std::fmt::Debug for DecryptedLogins {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DecryptedLogins")
            .field("count", &self.logins.len())
            .field("failed", &self.failed)
            .field("skipped", &self.skipped)
            .field("scheme", &self.scheme)
            .finish()
    }
}

/// Unlocks `key4.db` and decrypts every login. `Err(WrongPrimaryPassword)` means
/// the UI should ask for the primary password (try `""` first).
pub fn read(profile: &Path, primary_password: &str) -> Result<DecryptedLogins> {
    let text =
        std::fs::read_to_string(profile.join("logins.json")).map_err(|_| Error::NotFound("logins.json".into()))?;
    if !profile.join("key4.db").is_file() {
        return Err(Error::Unsupported(
            "this profile uses key3.db (Firefox 57 or older); export a CSV from about:logins instead".into(),
        ));
    }
    let snap = snapshot::open(profile, "key4.db")?;
    let keys = key4::unlock(&snap.conn, primary_password)?;
    log::info!("firefox import: key4.db uses {:?}; key sizes {:?}", keys.check_kind, keys.lengths());
    decrypt_all(&text, &keys)
}

pub fn decrypt_all(logins_json: &str, keys: &NssKeys) -> Result<DecryptedLogins> {
    let file: LoginsFile = serde_json::from_str(logins_json)?;
    let mut out = DecryptedLogins { logins: Vec::new(), failed: 0, skipped: 0, scheme: keys.check_kind };
    for raw in file.logins {
        if raw.hostname.starts_with("chrome://") {
            out.skipped += 1;
            continue;
        }
        let Some(origin) = normalize_origin(&raw.hostname) else {
            out.failed += 1;
            continue;
        };
        let (username, password): (Zeroizing<String>, Zeroizing<String>) = match (
            key4::decrypt_login_value(keys, &raw.encrypted_username),
            key4::decrypt_login_value(keys, &raw.encrypted_password),
        ) {
            (Ok(u), Ok(p)) => (u, p),
            _ => {
                out.failed += 1;
                continue;
            }
        };
        let ms = |v: Option<i64>| v.filter(|&t| t > 0).map(|t| t * 1000);
        out.logins.push(NewLogin {
            origin,
            action_origin: raw.form_submit_url.as_deref().and_then(normalize_origin),
            realm: raw.http_realm,
            username,
            password,
            username_field: raw.username_field.filter(|s| !s.is_empty()),
            password_field: raw.password_field.filter(|s| !s.is_empty()),
            guid: raw.guid,
            created_us: ms(raw.time_created),
            last_used_us: ms(raw.time_last_used),
            changed_us: ms(raw.time_password_changed),
            times_used: raw.times_used.unwrap_or(0),
        });
    }
    Ok(out)
}
