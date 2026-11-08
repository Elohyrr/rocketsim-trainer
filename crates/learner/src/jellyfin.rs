//! D6 — détection d'une lecture vidéo sur Jellyfin (API `GET /Sessions`).
//!
//! Client HTTP/1.1 minimal sur `std::net` (http seulement : l'URL est celle du Service interne du cluster).
//! Authentification : `Authorization: MediaBrowser Token="…"` (Jellyfin 10.11 refuse `X-Emby-Token`).
//! La clé n'est jamais journalisée.

use std::io::{Read, Write};
use std::net::{TcpStream, ToSocketAddrs};
use std::time::Duration;

const TIMEOUT: Duration = Duration::from_secs(5);
const MAX_BODY: usize = 4 << 20;

/// `true` si une session lit une vidéo (non en pause).
pub fn is_video_playing(sessions_json: &str) -> Result<bool, String> {
    let v: serde_json::Value = serde_json::from_str(sessions_json).map_err(|e| e.to_string())?;
    let list = v.as_array().ok_or("réponse /Sessions inattendue")?;
    Ok(list.iter().any(|s| {
        let Some(item) = s.get("NowPlayingItem") else {
            return false;
        };
        let video = item.get("MediaType").and_then(|m| m.as_str()) == Some("Video");
        let paused = s
            .pointer("/PlayState/IsPaused")
            .and_then(|p| p.as_bool())
            .unwrap_or(false);
        video && !paused
    }))
}

fn split_url(url: &str) -> Result<(String, String), String> {
    let rest = url
        .strip_prefix("http://")
        .ok_or("JELLYFIN_URL doit commencer par http:// (Service interne)")?;
    let (host, base) = rest.split_once('/').unwrap_or((rest, ""));
    let host = if host.contains(':') {
        host.to_string()
    } else {
        format!("{host}:80")
    };
    let base = base.trim_matches('/');
    let base = if base.is_empty() {
        String::new()
    } else {
        format!("/{base}")
    };
    Ok((host, base))
}

fn decode_chunked(mut b: &[u8]) -> Result<Vec<u8>, String> {
    let mut out = Vec::new();
    loop {
        let eol = b
            .windows(2)
            .position(|w| w == b"\r\n")
            .ok_or("chunk tronqué")?;
        let size = usize::from_str_radix(
            std::str::from_utf8(&b[..eol])
                .map_err(|e| e.to_string())?
                .split(';')
                .next()
                .unwrap_or("")
                .trim(),
            16,
        )
        .map_err(|e| e.to_string())?;
        b = &b[eol + 2..];
        if size == 0 {
            return Ok(out);
        }
        if b.len() < size + 2 {
            return Err("chunk tronqué".into());
        }
        out.extend_from_slice(&b[..size]);
        b = &b[size + 2..];
    }
}

/// Corps d'une réponse HTTP complète ; erreur si le statut n'est pas 200.
fn parse_response(raw: &[u8]) -> Result<String, String> {
    let split = raw
        .windows(4)
        .position(|w| w == b"\r\n\r\n")
        .ok_or("réponse HTTP invalide")?;
    let head = String::from_utf8_lossy(&raw[..split]).to_string();
    let status = head.lines().next().unwrap_or("");
    if !status.contains(" 200") {
        return Err(format!("statut HTTP : {status}"));
    }
    let body = &raw[split + 4..];
    let chunked = head.lines().any(|l| {
        l.to_ascii_lowercase().starts_with("transfer-encoding:")
            && l.to_ascii_lowercase().contains("chunked")
    });
    let body = if chunked {
        decode_chunked(body)?
    } else {
        body.to_vec()
    };
    String::from_utf8(body).map_err(|e| e.to_string())
}

pub struct Jellyfin {
    host: String,
    base: String,
    key: String,
}

impl Jellyfin {
    pub fn new(url: &str, key: &str) -> Result<Self, String> {
        let (host, base) = split_url(url)?;
        Ok(Self {
            host,
            base,
            key: key.to_string(),
        })
    }

    /// Variables `JELLYFIN_URL` et `JELLYFIN_API_KEY` ; `None` (pause désactivée) si la clé est absente ou vide.
    pub fn from_env() -> Option<Result<Self, String>> {
        let key = std::env::var("JELLYFIN_API_KEY").ok()?;
        if key.trim().is_empty() {
            return None;
        }
        let url = std::env::var("JELLYFIN_URL").unwrap_or_default();
        Some(Self::new(&url, key.trim()))
    }

    pub fn video_playing(&self) -> Result<bool, String> {
        let addr = self
            .host
            .to_socket_addrs()
            .map_err(|e| format!("résolution {} : {e}", self.host))?
            .next()
            .ok_or("aucune adresse")?;
        let mut c = TcpStream::connect_timeout(&addr, TIMEOUT).map_err(|e| e.to_string())?;
        c.set_read_timeout(Some(TIMEOUT)).ok();
        c.set_write_timeout(Some(TIMEOUT)).ok();
        let req = format!(
            "GET {}/Sessions HTTP/1.1\r\nHost: {}\r\nAuthorization: MediaBrowser Token=\"{}\"\r\nAccept: application/json\r\nConnection: close\r\n\r\n",
            self.base, self.host, self.key
        );
        c.write_all(req.as_bytes()).map_err(|e| e.to_string())?;
        let mut raw = Vec::new();
        c.take(MAX_BODY as u64)
            .read_to_end(&mut raw)
            .map_err(|e| e.to_string())?;
        is_video_playing(&parse_response(&raw)?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_a_playing_video_only() {
        assert!(!is_video_playing("[]").unwrap());
        assert!(!is_video_playing(r#"[{"Id":"a"}]"#).unwrap());
        let playing =
            r#"[{"NowPlayingItem":{"MediaType":"Video"},"PlayState":{"IsPaused":false}}]"#;
        assert!(is_video_playing(playing).unwrap());
        let paused = r#"[{"NowPlayingItem":{"MediaType":"Video"},"PlayState":{"IsPaused":true}}]"#;
        assert!(!is_video_playing(paused).unwrap());
        let music = r#"[{"NowPlayingItem":{"MediaType":"Audio"},"PlayState":{"IsPaused":false}}]"#;
        assert!(!is_video_playing(music).unwrap());
        assert!(is_video_playing("{}").is_err());
    }

    #[test]
    fn parses_plain_and_chunked_responses() {
        let plain = b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\n\r\n[]";
        assert_eq!(parse_response(plain).unwrap(), "[]");
        let chunked =
            b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n1\r\n[\r\n1\r\n]\r\n0\r\n\r\n";
        assert_eq!(parse_response(chunked).unwrap(), "[]");
        assert!(parse_response(b"HTTP/1.1 401 Unauthorized\r\n\r\n").is_err());
    }

    #[test]
    fn rejects_https_and_splits_urls() {
        assert!(Jellyfin::new("https://x", "k").is_err());
        assert_eq!(
            split_url("http://jellyfin.media.svc:8096/").unwrap(),
            ("jellyfin.media.svc:8096".into(), String::new())
        );
        assert_eq!(
            split_url("http://h/jf/").unwrap(),
            ("h:80".into(), "/jf".into())
        );
    }

    #[test]
    fn talks_to_a_fake_server_with_the_mediabrowser_header() {
        let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = l.local_addr().unwrap().port();
        let h = std::thread::spawn(move || {
            let (mut s, _) = l.accept().unwrap();
            let mut buf = [0u8; 2048];
            let n = s.read(&mut buf).unwrap();
            let req = String::from_utf8_lossy(&buf[..n]).to_string();
            let body =
                r#"[{"NowPlayingItem":{"MediaType":"Video"},"PlayState":{"IsPaused":false}}]"#;
            write!(
                s,
                "HTTP/1.1 200 OK\r\nContent-Length: {}\r\n\r\n{body}",
                body.len()
            )
            .unwrap();
            req
        });
        let j = Jellyfin::new(&format!("http://127.0.0.1:{port}"), "secret").unwrap();
        assert!(j.video_playing().unwrap());
        let req = h.join().unwrap();
        assert!(req.starts_with("GET /Sessions "));
        assert!(req.contains("Authorization: MediaBrowser Token=\"secret\""));
        assert!(!req.contains("X-Emby-Token"));
    }
}
