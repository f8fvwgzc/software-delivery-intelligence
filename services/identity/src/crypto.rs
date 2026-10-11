//! Everything derived from `SDI_SESSION_SECRET`: opaque session tokens and
//! their stored hash, the signed OAuth `state`, and the AEAD that seals
//! OAuth tokens and reasoner keys at rest (CONTRACT.md §18, §19).

use aes_gcm::{
    Aes256Gcm,
    aead::{Aead, AeadCore, KeyInit, OsRng, rand_core::RngCore},
};
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use chrono::{DateTime, Duration, Utc};
use hmac::{Hmac, Mac};
use serde::{Deserialize, Serialize};
use sha2::Sha256;

type HmacSha256 = Hmac<Sha256>;

const NONCE_LEN: usize = 12;
/// A login must complete within this long after `BeginLogin`.
pub const STATE_TTL: Duration = Duration::minutes(10);

#[derive(Debug, thiserror::Error)]
pub enum CryptoError {
    #[error("sealed data is malformed")]
    Malformed,
    #[error("sealed data does not authenticate")]
    Tampered,
    #[error("state expired")]
    Expired,
}

/// Purpose-separated keys, so the hash of a session token can never be
/// mistaken for a MAC and the MAC key never encrypts.
#[derive(Clone)]
pub struct Keys {
    seal: [u8; 32],
    sign: [u8; 32],
    hash: [u8; 32],
}

impl std::fmt::Debug for Keys {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Keys(..)")
    }
}

fn derive(secret: &[u8], purpose: &str) -> [u8; 32] {
    let mut mac = <HmacSha256 as Mac>::new_from_slice(secret).expect("hmac accepts any key length");
    mac.update(purpose.as_bytes());
    mac.finalize().into_bytes().into()
}

/// What the signed OAuth `state` carries.
#[derive(Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct LoginState {
    pub nonce: String,
    pub return_to: String,
    pub expires_at: DateTime<Utc>,
}

impl Keys {
    pub fn from_secret(secret: &[u8]) -> Self {
        Self {
            seal: derive(secret, "sdi/identity/seal/v1"),
            sign: derive(secret, "sdi/identity/sign/v1"),
            hash: derive(secret, "sdi/identity/token-hash/v1"),
        }
    }

    /// 32 random bytes, base64url: the opaque cookie value.
    pub fn new_token() -> String {
        let mut bytes = [0u8; 32];
        OsRng.fill_bytes(&mut bytes);
        URL_SAFE_NO_PAD.encode(bytes)
    }

    /// Keyed hash under which a session is stored and looked up.
    pub fn hash_token(&self, token: &str) -> Vec<u8> {
        let mut mac =
            <HmacSha256 as Mac>::new_from_slice(&self.hash).expect("hmac accepts any key length");
        mac.update(token.as_bytes());
        mac.finalize().into_bytes().to_vec()
    }

    /// AES-256-GCM with a fresh random nonce: `nonce || ciphertext`.
    pub fn seal(&self, plaintext: &[u8]) -> Vec<u8> {
        let cipher = Aes256Gcm::new_from_slice(&self.seal).expect("32-byte key");
        let nonce = Aes256Gcm::generate_nonce(&mut OsRng);
        let mut out = nonce.to_vec();
        let ciphertext = cipher
            .encrypt(&nonce, plaintext)
            .expect("aes-gcm encryption cannot fail for in-memory data");
        out.extend_from_slice(&ciphertext);
        out
    }

    pub fn open(&self, sealed: &[u8]) -> Result<Vec<u8>, CryptoError> {
        if sealed.len() < NONCE_LEN + 16 {
            return Err(CryptoError::Malformed);
        }
        let (nonce, ciphertext) = sealed.split_at(NONCE_LEN);
        let cipher = Aes256Gcm::new_from_slice(&self.seal).expect("32-byte key");
        cipher
            .decrypt(<&aes_gcm::aead::Nonce<Aes256Gcm>>::from(nonce), ciphertext)
            .map_err(|_| CryptoError::Tampered)
    }

    pub fn open_str(&self, sealed: &[u8]) -> Result<String, CryptoError> {
        String::from_utf8(self.open(sealed)?).map_err(|_| CryptoError::Malformed)
    }

    /// `base64url(json) . base64url(hmac)`: the OAuth `state`.
    pub fn sign_state(&self, return_to: &str) -> String {
        let state = LoginState {
            nonce: Self::new_token(),
            return_to: return_to.to_owned(),
            expires_at: Utc::now() + STATE_TTL,
        };
        let payload = serde_json::to_vec(&state).expect("state serialises");
        let encoded = URL_SAFE_NO_PAD.encode(&payload);
        let tag = self.mac(encoded.as_bytes());
        format!("{encoded}.{}", URL_SAFE_NO_PAD.encode(tag))
    }

    pub fn verify_state(&self, state: &str) -> Result<LoginState, CryptoError> {
        let (encoded, tag) = state.split_once('.').ok_or(CryptoError::Malformed)?;
        let tag = URL_SAFE_NO_PAD
            .decode(tag)
            .map_err(|_| CryptoError::Malformed)?;
        let mut mac =
            <HmacSha256 as Mac>::new_from_slice(&self.sign).expect("hmac accepts any key length");
        mac.update(encoded.as_bytes());
        mac.verify_slice(&tag).map_err(|_| CryptoError::Tampered)?;
        let payload = URL_SAFE_NO_PAD
            .decode(encoded)
            .map_err(|_| CryptoError::Malformed)?;
        let state: LoginState =
            serde_json::from_slice(&payload).map_err(|_| CryptoError::Malformed)?;
        if state.expires_at < Utc::now() {
            return Err(CryptoError::Expired);
        }
        Ok(state)
    }

    fn mac(&self, data: &[u8]) -> Vec<u8> {
        let mut mac =
            <HmacSha256 as Mac>::new_from_slice(&self.sign).expect("hmac accepts any key length");
        mac.update(data);
        mac.finalize().into_bytes().to_vec()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn keys() -> Keys {
        Keys::from_secret(b"0123456789abcdef0123456789abcdef")
    }

    #[test]
    fn seal_and_open_round_trip_and_detect_tampering() {
        let keys = keys();
        let sealed = keys.seal(b"gho_secret");
        assert_eq!(keys.open_str(&sealed).unwrap(), "gho_secret");
        // Every seal uses a fresh nonce.
        assert_ne!(sealed, keys.seal(b"gho_secret"));

        let mut tampered = sealed.clone();
        let last = tampered.len() - 1;
        tampered[last] ^= 1;
        assert!(matches!(keys.open(&tampered), Err(CryptoError::Tampered)));
        assert!(matches!(keys.open(b"short"), Err(CryptoError::Malformed)));

        let other = Keys::from_secret(b"another-secret-of-thirty-two-byte!");
        assert!(other.open(&sealed).is_err());
    }

    #[test]
    fn state_signs_and_verifies_but_not_when_altered() {
        let keys = keys();
        let state = keys.sign_state("/projects");
        let parsed = keys.verify_state(&state).unwrap();
        assert_eq!(parsed.return_to, "/projects");
        assert!(parsed.expires_at > Utc::now());

        let (payload, tag) = state.split_once('.').unwrap();
        let forged_payload = URL_SAFE_NO_PAD.encode(
            serde_json::to_vec(&LoginState {
                nonce: "x".into(),
                return_to: "https://evil.example".into(),
                expires_at: Utc::now() + STATE_TTL,
            })
            .unwrap(),
        );
        assert!(matches!(
            keys.verify_state(&format!("{forged_payload}.{tag}")),
            Err(CryptoError::Tampered)
        ));
        assert!(matches!(
            keys.verify_state(payload),
            Err(CryptoError::Malformed)
        ));
        assert!(
            Keys::from_secret(b"another-secret-of-thirty-two-byte!")
                .verify_state(&state)
                .is_err()
        );
    }

    #[test]
    fn expired_state_is_rejected() {
        let keys = keys();
        let state = LoginState {
            nonce: "n".into(),
            return_to: "/".into(),
            expires_at: Utc::now() - Duration::seconds(1),
        };
        let encoded = URL_SAFE_NO_PAD.encode(serde_json::to_vec(&state).unwrap());
        let signed = format!(
            "{encoded}.{}",
            URL_SAFE_NO_PAD.encode(keys.mac(encoded.as_bytes()))
        );
        assert!(matches!(
            keys.verify_state(&signed),
            Err(CryptoError::Expired)
        ));
    }

    #[test]
    fn token_hash_is_keyed_and_stable() {
        let keys = keys();
        let token = Keys::new_token();
        assert!(token.len() >= 40);
        assert_eq!(keys.hash_token(&token), keys.hash_token(&token));
        assert_ne!(keys.hash_token(&token), keys.hash_token("other"));
        assert_ne!(
            keys.hash_token(&token),
            Keys::from_secret(b"another-secret-of-thirty-two-byte!").hash_token(&token)
        );
    }
}
