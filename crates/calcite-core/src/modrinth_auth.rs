//! Signing in to Modrinth from a desktop app ("launcher protocol 2").
//!
//! Calcite opens modrinth.com in the player's browser with a one-time
//! public key, a nonce and a loopback port. After sign-in the site redirects
//! the browser to `http://127.0.0.1:<port>/?code=…&nonce=…`, where `code` is
//! the session token sealed to our key. Only this process can open it.
//!
//! Ported from Modrinth App (GPL-3.0, `apps/app/src/api/oauth_utils`).
//!
//! Sealed wire format, base64url without padding:
//! `version (0x02) || spki_len (u16 BE) || spki || iv (12) || ciphertext+tag`.
//! The AES-256-GCM key is HKDF-SHA256 (no salt, info `modrinth-app-auth-v2`)
//! over the P-256 ECDH shared secret. Plaintext is `{nonce}.{sessionToken}`.

use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};
use std::time::Duration;

use aes_gcm::aead::Aead;
use aes_gcm::{Aes256Gcm, KeyInit, Nonce};
use anyhow::{Context, anyhow};
use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use hkdf::Hkdf;
use p256::ecdh::diffie_hellman;
use p256::elliptic_curve::Generate;
use p256::pkcs8::{DecodePublicKey, EncodePublicKey};
use p256::{PublicKey, SecretKey};
use rand::Rng;
use sha2::Sha256;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::oneshot;

const SEAL_VERSION: u8 = 2;
const PROTOCOL: &str = "2";
const PROTOCOL_INFO: &[u8] = b"modrinth-app-auth-v2";
const MAX_SEALED_LEN: usize = 4096;
const MAX_EPHEMERAL_SPKI_LEN: usize = 256;
const MAX_REQUEST: usize = 16 * 1024;

/// A one-time key pair and nonce for a single sign-in.
pub struct Seal {
    secret: SecretKey,
    nonce: String,
}

impl Seal {
    pub fn generate() -> Self {
        let mut rng = rand::rng();
        let secret = SecretKey::generate_from_rng(&mut rng);
        let mut nonce_bytes = [0u8; 16];
        rng.fill_bytes(&mut nonce_bytes);
        Seal {
            secret,
            nonce: URL_SAFE_NO_PAD.encode(nonce_bytes),
        }
    }

    pub fn nonce(&self) -> &str {
        &self.nonce
    }

    pub fn public_key(&self) -> anyhow::Result<String> {
        let der = self
            .secret
            .public_key()
            .to_public_key_der()
            .context("couldn't encode the sign-in key")?;
        Ok(URL_SAFE_NO_PAD.encode(der.as_bytes()))
    }

    /// Open the sealed session token the site sent back.
    pub fn open(&self, sealed: &str) -> anyhow::Result<String> {
        let invalid = || anyhow!("Modrinth sent back a sign-in Calcite couldn't read. Try again.");
        if sealed.len() > MAX_SEALED_LEN {
            return Err(invalid());
        }
        let bytes = URL_SAFE_NO_PAD.decode(sealed).map_err(|_| invalid())?;
        if bytes.first() != Some(&SEAL_VERSION) || bytes.len() < 3 {
            return Err(invalid());
        }
        let spki_len = u16::from_be_bytes([bytes[1], bytes[2]]) as usize;
        let header_len = 3 + spki_len + 12;
        if spki_len == 0 || spki_len > MAX_EPHEMERAL_SPKI_LEN || bytes.len() < header_len + 16 {
            return Err(invalid());
        }
        let spki = &bytes[3..3 + spki_len];
        let mut iv = [0u8; 12];
        iv.copy_from_slice(&bytes[3 + spki_len..header_len]);
        let ephemeral = PublicKey::from_public_key_der(spki).map_err(|_| invalid())?;
        let cipher = cipher(&self.secret, &ephemeral).ok_or_else(invalid)?;
        let plain = cipher
            .decrypt(&Nonce::from(iv), &bytes[header_len..])
            .map_err(|_| invalid())?;
        let plain = std::str::from_utf8(&plain).map_err(|_| invalid())?;
        let (nonce, token) = plain.split_once('.').ok_or_else(invalid)?;
        if nonce != self.nonce || !is_session_token(token) {
            return Err(invalid());
        }
        Ok(token.to_string())
    }
}

fn cipher(secret: &SecretKey, public: &PublicKey) -> Option<Aes256Gcm> {
    let shared = diffie_hellman(secret.to_nonzero_scalar(), public.as_affine());
    let mut key = [0u8; 32];
    Hkdf::<Sha256>::new(None, shared.raw_secret_bytes())
        .expand(PROTOCOL_INFO, &mut key)
        .ok()?;
    Aes256Gcm::new_from_slice(&key).ok()
}

fn is_session_token(token: &str) -> bool {
    token.strip_prefix("mra_").is_some_and(|rest| {
        (32..=128).contains(&rest.len()) && rest.chars().all(|c| c.is_ascii_alphanumeric())
    })
}

/// The page to open in the browser.
pub fn request_url(begin_url: &str, reply: SocketAddr, seal: &Seal) -> anyhow::Result<String> {
    let mut url = url::Url::parse(begin_url).context("couldn't build the sign-in address")?;
    url.query_pairs_mut()
        .append_pair("launcher", "true")
        .append_pair("protocol", PROTOCOL)
        .append_pair("key", &seal.public_key()?)
        .append_pair("nonce", seal.nonce())
        .append_pair("ipver", if reply.is_ipv4() { "4" } else { "6" })
        .append_pair("port", &reply.port().to_string());
    Ok(url.into())
}

/// A loopback listener waiting for the browser to come back with the code.
pub struct Reply {
    listener: TcpListener,
    nonce: String,
}

impl Reply {
    /// Listen on an ephemeral loopback port (IPv4 first, then IPv6), as
    /// RFC 8252 recommends.
    pub async fn bind(nonce: &str) -> anyhow::Result<Reply> {
        let listener = TcpListener::bind(
            &[
                SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 0),
                SocketAddr::new(IpAddr::V6(Ipv6Addr::LOCALHOST), 0),
            ][..],
        )
        .await
        .context("couldn't open a local port for the sign-in reply")?;
        Ok(Reply {
            listener,
            nonce: nonce.to_string(),
        })
    }

    pub fn addr(&self) -> anyhow::Result<SocketAddr> {
        Ok(self.listener.local_addr()?)
    }

    /// Wait for the browser's redirect. `None` if cancelled or timed out.
    pub async fn wait(
        self,
        cancel: oneshot::Receiver<()>,
        timeout: Duration,
    ) -> anyhow::Result<Option<String>> {
        let accept = async {
            loop {
                let Ok((stream, _)) = self.listener.accept().await else {
                    continue;
                };
                if let Some(code) = answer(stream, &self.nonce).await {
                    return code;
                }
            }
        };
        tokio::select! {
            code = accept => Ok(Some(code)),
            _ = cancel => Ok(None),
            _ = tokio::time::sleep(timeout) => Ok(None),
        }
    }
}

/// Serve one request. Returns the code when it's the redirect we expect.
async fn answer(mut stream: TcpStream, nonce: &str) -> Option<String> {
    let mut buf = Vec::with_capacity(1024);
    let read = tokio::time::timeout(Duration::from_secs(5), async {
        let mut chunk = [0u8; 1024];
        while !buf.windows(4).any(|w| w == b"\r\n\r\n") && buf.len() < MAX_REQUEST {
            let n = stream.read(&mut chunk).await.ok()?;
            if n == 0 {
                break;
            }
            buf.extend_from_slice(&chunk[..n]);
        }
        Some(())
    })
    .await;
    if !matches!(read, Ok(Some(()))) {
        return None;
    }
    let head = String::from_utf8_lossy(&buf);
    let request_line = head.lines().next().unwrap_or_default();
    let mut parts = request_line.split_whitespace();
    let (method, target) = (
        parts.next().unwrap_or_default(),
        parts.next().unwrap_or_default(),
    );
    let found = if method == "GET" {
        parse_callback(target, nonce)
    } else {
        None
    };
    let (status, title, message) = match &found {
        Some(_) => (
            "200 OK",
            "Signed in",
            "You're signed in to Modrinth. You can close this tab and go back to Calcite.",
        ),
        None => (
            "400 Bad Request",
            "Something went wrong",
            "This sign-in link didn't work. Go back to Calcite and try again.",
        ),
    };
    let body = page(title, message);
    let response = format!(
        "HTTP/1.1 {status}\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\nCache-Control: no-store\r\n\r\n{body}",
        body.len()
    );
    let _ = stream.write_all(response.as_bytes()).await;
    let _ = stream.shutdown().await;
    found
}

/// The code from `/?code=…&nonce=…`, if the nonce matches.
fn parse_callback(target: &str, expected_nonce: &str) -> Option<String> {
    let query = target.split_once('?').map(|(_, q)| q)?;
    let (mut code, mut nonce) = (None, None);
    for (key, value) in url::form_urlencoded::parse(query.as_bytes()) {
        if value.len() > MAX_SEALED_LEN {
            return None;
        }
        match key.as_ref() {
            "code" => code = Some(value.into_owned()),
            "nonce" => nonce = Some(value.into_owned()),
            _ => {}
        }
    }
    (nonce.as_deref() == Some(expected_nonce))
        .then_some(code)
        .flatten()
        .filter(|c| !c.is_empty())
}

fn page(title: &str, message: &str) -> String {
    format!(
        r#"<!doctype html><html><head><meta charset="utf-8"><meta name="viewport" content="width=device-width,initial-scale=1"><title>{title} · Calcite</title>
<style>
:root{{color-scheme:light dark;--bg:#f4f1ec;--fg:#1c1a17;--muted:#6b655c;--accent:#d99a2b}}
@media (prefers-color-scheme:dark){{:root{{--bg:#141416;--fg:#ecebe8;--muted:#9a978f}}}}
body{{margin:0;min-height:100vh;display:grid;place-items:center;background:var(--bg);color:var(--fg);font:15px/1.5 -apple-system,BlinkMacSystemFont,"Segoe UI",Inter,sans-serif}}
main{{max-width:360px;padding:24px;text-align:center}}
.mark{{width:40px;height:40px;margin:0 auto 20px;background:var(--accent);clip-path:polygon(50% 0,100% 30%,82% 100%,18% 100%,0 30%)}}
h1{{font-size:20px;font-weight:600;margin:0 0 8px}}p{{margin:0;color:var(--muted)}}
</style></head><body><main><div class="mark"></div><h1>{title}</h1><p>{message}</p></main></body></html>"#
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    /// What modrinth.com does: seal `{nonce}.{token}` to our public key.
    fn seal_like_the_site(public_b64: &str, nonce: &str, token: &str) -> String {
        let ours =
            PublicKey::from_public_key_der(&URL_SAFE_NO_PAD.decode(public_b64).unwrap()).unwrap();
        let ephemeral = SecretKey::generate_from_rng(&mut rand::rng());
        let cipher = cipher(&ephemeral, &ours).unwrap();
        let iv = [7u8; 12];
        let ct = cipher
            .encrypt(&Nonce::from(iv), format!("{nonce}.{token}").as_bytes())
            .unwrap();
        let spki = ephemeral.public_key().to_public_key_der().unwrap();
        let mut out = vec![SEAL_VERSION];
        out.extend_from_slice(&(spki.as_bytes().len() as u16).to_be_bytes());
        out.extend_from_slice(spki.as_bytes());
        out.extend_from_slice(&iv);
        out.extend_from_slice(&ct);
        URL_SAFE_NO_PAD.encode(out)
    }

    const TOKEN: &str = "mra_abcdefghijklmnopqrstuvwxyz0123456789";

    #[test]
    fn opens_what_the_site_seals() {
        let seal = Seal::generate();
        let sealed = seal_like_the_site(&seal.public_key().unwrap(), seal.nonce(), TOKEN);
        assert_eq!(seal.open(&sealed).unwrap(), TOKEN);
    }

    #[test]
    fn rejects_another_nonce_or_key() {
        let seal = Seal::generate();
        let wrong_nonce = seal_like_the_site(&seal.public_key().unwrap(), "other", TOKEN);
        assert!(seal.open(&wrong_nonce).is_err());
        let other = Seal::generate();
        let for_other = seal_like_the_site(&other.public_key().unwrap(), seal.nonce(), TOKEN);
        assert!(seal.open(&for_other).is_err());
        assert!(seal.open("not base64!").is_err());
    }

    #[test]
    fn builds_the_request_url() {
        let seal = Seal::generate();
        let url = request_url(
            "https://modrinth.com/auth/sign-in",
            "127.0.0.1:4567".parse().unwrap(),
            &seal,
        )
        .unwrap();
        let parsed = url::Url::parse(&url).unwrap();
        let q: std::collections::HashMap<_, _> = parsed.query_pairs().into_owned().collect();
        assert_eq!(q["launcher"], "true");
        assert_eq!(q["protocol"], "2");
        assert_eq!(q["port"], "4567");
        assert_eq!(q["ipver"], "4");
        assert_eq!(q["nonce"], seal.nonce());
    }

    #[tokio::test]
    async fn the_loopback_reply_returns_the_code() {
        let reply = Reply::bind("n0nce").await.unwrap();
        let addr = reply.addr().unwrap();
        let (_cancel_tx, cancel_rx) = oneshot::channel();
        let wait = tokio::spawn(reply.wait(cancel_rx, Duration::from_secs(10)));
        // A stray request (wrong nonce) gets an error page and is ignored.
        for target in [
            "/favicon.ico",
            "/?code=x&nonce=wrong",
            "/?code=SEALED&nonce=n0nce",
        ] {
            let mut s = TcpStream::connect(addr).await.unwrap();
            s.write_all(format!("GET {target} HTTP/1.1\r\nHost: x\r\n\r\n").as_bytes())
                .await
                .unwrap();
            let mut response = String::new();
            s.read_to_string(&mut response).await.unwrap();
            assert!(response.starts_with("HTTP/1.1"), "{response}");
        }
        assert_eq!(wait.await.unwrap().unwrap(), Some("SEALED".to_string()));
    }

    #[tokio::test]
    async fn cancelling_stops_the_wait() {
        let reply = Reply::bind("n").await.unwrap();
        let (cancel_tx, cancel_rx) = oneshot::channel();
        let wait = tokio::spawn(reply.wait(cancel_rx, Duration::from_secs(10)));
        cancel_tx.send(()).unwrap();
        assert_eq!(wait.await.unwrap().unwrap(), None);
    }
}
