//! Extension update checks (Omaha "update2" protocol used by both stores).

use std::cmp::Ordering;

use serde::Serialize;

/// Builds an update-check URL for several extensions at once.
pub fn check_url(base: &str, prodversion: &str, installed: &[(String, String)]) -> String {
    let mut url = format!("{base}?response=updatecheck&prodversion={prodversion}&acceptformat=crx3");
    for (id, version) in installed {
        let x = format!("id={id}&v={version}&uc");
        let enc: String = url::form_urlencoded::byte_serialize(x.as_bytes()).collect();
        url.push_str("&x=");
        url.push_str(&enc);
    }
    url
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AvailableUpdate {
    pub id: String,
    pub version: String,
    pub codebase: String,
    /// Hex SHA-256 of the package, when the server provides it.
    pub sha256: Option<String>,
}

fn attr(tag: &str, name: &str) -> Option<String> {
    let needle = format!("{name}=");
    let mut from = 0;
    while let Some(i) = tag[from..].find(&needle) {
        let at = from + i;
        let boundary = at == 0 || tag.as_bytes()[at - 1].is_ascii_whitespace();
        let rest = &tag[at + needle.len()..];
        if boundary {
            let quote = rest.chars().next()?;
            if quote == '"' || quote == '\'' {
                let body = &rest[1..];
                let end = body.find(quote)?;
                return Some(body[..end].replace("&amp;", "&"));
            }
            return None;
        }
        from = at + needle.len();
    }
    None
}

/// Parses the `<gupdate>` response into the updates that are actually available.
pub fn parse_response(xml: &str) -> Vec<AvailableUpdate> {
    let mut out = Vec::new();
    let mut current_app: Option<String> = None;
    let mut rest = xml;
    while let Some(start) = rest.find('<') {
        let Some(end) = rest[start..].find('>') else { break };
        let tag = &rest[start + 1..start + end];
        rest = &rest[start + end + 1..];
        let name = tag.split(|c: char| c.is_whitespace() || c == '/').next().unwrap_or("");
        match name {
            "app" => current_app = attr(tag, "appid"),
            "updatecheck" => {
                let (Some(id), Some(status)) = (current_app.clone(), attr(tag, "status")) else { continue };
                if status != "ok" {
                    continue;
                }
                if let (Some(version), Some(codebase)) = (attr(tag, "version"), attr(tag, "codebase"))
                    && codebase.starts_with("https://")
                {
                    out.push(AvailableUpdate { id, version, codebase, sha256: attr(tag, "hash_sha256") });
                }
            }
            _ => {}
        }
    }
    out
}

/// Compares dotted numeric versions (`1.10.0` > `1.9`).
pub fn compare_versions(a: &str, b: &str) -> Ordering {
    let parse = |s: &str| -> Vec<u64> { s.split('.').map(|p| p.trim().parse().unwrap_or(0)).collect() };
    let (va, vb) = (parse(a), parse(b));
    for i in 0..va.len().max(vb.len()) {
        match va.get(i).unwrap_or(&0).cmp(vb.get(i).unwrap_or(&0)) {
            Ordering::Equal => continue,
            o => return o,
        }
    }
    Ordering::Equal
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_update_response() {
        let xml = r#"<?xml version="1.0" encoding="UTF-8"?><gupdate xmlns="http://www.google.com/update2/response" protocol="2.0" server="prod">
          <daystart elapsed_days="6481" elapsed_seconds="44010"/>
          <app appid="ddkjiahejlhfcafbddmgiahcphecmpfh" cohort="1::" status="ok">
            <updatecheck codebase="https://clients2.googleusercontent.com/crx/blobs/abc/x.crx?a=1&amp;b=2" fp="1.x" hash_sha256="00ff" protocol="3.1" size="123" status="ok" version="2025.1.1.1"/>
          </app>
          <app appid="eimadpbcbfnmbkopoojfekhnkhdbieeh" status="ok"><updatecheck status="noupdate"/></app>
          <app appid="xxx" status="error-unknownApplication"/>
        </gupdate>"#;
        let u = parse_response(xml);
        assert_eq!(u.len(), 1);
        assert_eq!(u[0].id, "ddkjiahejlhfcafbddmgiahcphecmpfh");
        assert_eq!(u[0].version, "2025.1.1.1");
        assert_eq!(u[0].codebase, "https://clients2.googleusercontent.com/crx/blobs/abc/x.crx?a=1&b=2");
        assert_eq!(u[0].sha256.as_deref(), Some("00ff"));
        assert!(parse_response("garbage").is_empty());
    }

    #[test]
    fn versions() {
        assert_eq!(compare_versions("1.10.0", "1.9"), Ordering::Greater);
        assert_eq!(compare_versions("1.0", "1"), Ordering::Equal);
        assert_eq!(compare_versions("2025.1.1", "2025.1.2"), Ordering::Less);
    }

    #[test]
    fn check_urls() {
        let u = check_url("https://x/crx", "140.0", &[("aaa".into(), "1.0".into()), ("bbb".into(), "2".into())]);
        assert_eq!(
            u,
            "https://x/crx?response=updatecheck&prodversion=140.0&acceptformat=crx3&x=id%3Daaa%26v%3D1.0%26uc&x=id%3Dbbb%26v%3D2%26uc"
        );
    }
}
