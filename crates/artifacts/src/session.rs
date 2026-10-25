use axum::http::HeaderMap;
use axum::http::header::COOKIE;
use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use sha2::{Digest, Sha256};

pub const SESSION_COOKIE: &str = "myc_artifacts";
pub const SESSION_DAYS: i64 = 30;

const BLOCK: usize = 64;

pub fn cookie<'a>(headers: &'a HeaderMap, name: &str) -> Option<&'a str> {
    let header = headers.get(COOKIE)?.to_str().ok()?;

    header.split(';').filter_map(|pair| pair.trim().split_once('=')).find(|(key, _)| *key == name).map(|(_, value)| value)
}

pub fn random_token() -> String {
    URL_SAFE_NO_PAD.encode(rand::random::<[u8; 32]>())
}

fn hmac_sha256(key: &[u8], message: &[u8]) -> Vec<u8> {
    let mut block = [0u8; BLOCK];
    block[..key.len()].copy_from_slice(key);

    let inner_pad: Vec<u8> = block.iter().map(|byte| byte ^ 0x36).collect();
    let outer_pad: Vec<u8> = block.iter().map(|byte| byte ^ 0x5c).collect();

    let mut inner = Sha256::new();
    inner.update(&inner_pad);
    inner.update(message);
    let inner_digest = inner.finalize().to_vec();

    let mut outer = Sha256::new();
    outer.update(&outer_pad);
    outer.update(&inner_digest);

    outer.finalize().to_vec()
}

fn same(left: &[u8], right: &[u8]) -> bool {
    left.len() == right.len() && left.iter().zip(right).fold(0u8, |difference, (a, b)| difference | (a ^ b)) == 0
}

pub struct Sessions {
    key: Vec<u8>,
}

impl Sessions {
    pub fn new(secret: &str) -> Sessions {
        Sessions { key: Sha256::digest(format!("mycelium-artifact-session:{secret}").as_bytes()).to_vec() }
    }

    pub fn issue(&self, now: i64, subject: &str) -> String {
        let claims = format!("{}.{}", now + SESSION_DAYS * 86400, URL_SAFE_NO_PAD.encode(subject));
        let mac = URL_SAFE_NO_PAD.encode(hmac_sha256(&self.key, claims.as_bytes()));

        format!("{claims}.{mac}")
    }

    pub fn subject(&self, value: &str, now: i64) -> Option<String> {
        let (claims, mac) = value.rsplit_once('.')?;
        let (expiry, subject) = claims.split_once('.')?;
        let mac = URL_SAFE_NO_PAD.decode(mac).ok()?;
        let expiry_time = expiry.parse::<i64>().ok()?;
        let subject = String::from_utf8(URL_SAFE_NO_PAD.decode(subject).ok()?).ok()?;

        (same(&hmac_sha256(&self.key, claims.as_bytes()), &mac) && expiry_time > now).then_some(subject)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hmac_matches_the_rfc_4231_first_test_vector() {
        let mac = hmac_sha256(&[0x0b; 20], b"Hi There");
        let hex: String = mac.iter().map(|byte| format!("{byte:02x}")).collect();

        assert_eq!(hex, "b0344c61d8db38535ca8afceaf0bf12b881dc200c9833da726e9376c2e32cff7");
    }

    #[test]
    fn a_session_is_valid_until_it_expires_and_not_under_another_key() {
        let sessions = Sessions::new("one");
        let value = sessions.issue(1000, "ada");

        assert_eq!(sessions.subject(&value, 1001).as_deref(), Some("ada"));
        assert_eq!(sessions.subject(&value, 1000 + SESSION_DAYS * 86400), None);
        assert_eq!(Sessions::new("two").subject(&value, 1001), None);
        assert_eq!(sessions.subject("nonsense", 1001), None);
    }

    #[test]
    fn a_session_cannot_be_moved_to_another_subject() {
        let sessions = Sessions::new("one");
        let value = sessions.issue(1000, "ada");
        let (claims, mac) = value.rsplit_once('.').unwrap();
        let (expiry, _) = claims.split_once('.').unwrap();
        let forged = format!("{expiry}.{}.{mac}", URL_SAFE_NO_PAD.encode("grace"));

        assert_eq!(sessions.subject(&forged, 1001), None);
    }
}
