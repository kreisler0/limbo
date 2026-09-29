//! Host-side HTTP (CRX downloads, update checks, Google suggestions) through
//! schannel and the Windows certificate store. Blocking: call from worker threads.

use std::time::Duration;

use ureq::tls::{RootCerts, TlsConfig, TlsProvider};

pub fn agent() -> ureq::Agent {
    ureq::Agent::config_builder()
        .tls_config(
            TlsConfig::builder().provider(TlsProvider::NativeTls).root_certs(RootCerts::PlatformVerifier).build(),
        )
        .timeout_global(Some(Duration::from_secs(30)))
        .user_agent(format!("Limbo/{}", env!("CARGO_PKG_VERSION")))
        .build()
        .new_agent()
}

/// GET returning the body, capped at `limit` bytes.
pub fn get_bytes(agent: &ureq::Agent, url: &str, limit: u64) -> Result<Vec<u8>, String> {
    let mut resp = agent.get(url).call().map_err(|e| e.to_string())?;
    resp.body_mut().with_config().limit(limit).read_to_vec().map_err(|e| e.to_string())
}

pub fn get_string(agent: &ureq::Agent, url: &str, timeout: Duration) -> Result<String, String> {
    let mut resp = agent.get(url).config().timeout_global(Some(timeout)).build().call().map_err(|e| e.to_string())?;
    resp.body_mut().with_config().limit(4 * 1024 * 1024).read_to_string().map_err(|e| e.to_string())
}
