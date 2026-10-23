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

    pub fn issue(&self, now: i64) -> String {
        let expiry = (now + SESSION_DAYS * 86400).to_string();
        let mac = URL_SAFE_NO_PAD.encode(hmac_sha256(&self.key, expiry.as_bytes()));

        format!("{expiry}.{mac}")
    }

    pub fn valid(&self, value: &str, now: i64) -> bool {
        let Some((expiry, mac)) = value.split_once('.') else { return false };
        let Ok(mac) = URL_SAFE_NO_PAD.decode(mac) else { return false };
        let Ok(expiry_time) = expiry.parse::<i64>() else { return false };

        same(&hmac_sha256(&self.key, expiry.as_bytes()), &mac) && expiry_time > now
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
        let value = sessions.issue(1000);

        assert!(sessions.valid(&value, 1001));
        assert!(!sessions.valid(&value, 1000 + SESSION_DAYS * 86400));
        assert!(!Sessions::new("two").valid(&value, 1001));
        assert!(!sessions.valid("nonsense", 1001));
    }
}
