//! Encrypting the Google refresh token at rest (specification §6.2 step 4).
//!
//! The refresh token is the one secret in this system that grants standing
//! access to somebody's photograph library, and it lives in a SQLite file on a
//! NAS. §9.2 rule 4 keeps it out of the repository and out of the photo
//! library; this keeps it unreadable to anyone who walks off with the database.
//!
//! **A missing key is an error, never a fallback.** Quietly storing the token in
//! the clear because nobody set `GOOGLE_REFRESH_TOKEN_ENCRYPTION_KEY` would make
//! the column name a lie and would be discovered, at the earliest, by whoever
//! read the database.

use crate::error::Error;
use chacha20poly1305::aead::{Aead, AeadCore, KeyInit, OsRng};
use chacha20poly1305::{ChaCha20Poly1305, Key, Nonce};

/// The environment variable holding the key, hex-encoded.
pub const KEY_VAR: &str = "GOOGLE_REFRESH_TOKEN_ENCRYPTION_KEY";

/// Bytes of key material. ChaCha20 takes exactly this many.
const KEY_BYTES: usize = 32;

/// Prefix on every stored ciphertext, so a future change of cipher can be told
/// from this one rather than guessed at.
const VERSION: &str = "v1";

/// A key read from the environment.
///
/// Held as the cipher rather than as bytes, so the key material is not sitting
/// in a `String` waiting to be logged by accident.
pub struct TokenCipher {
    cipher: ChaCha20Poly1305,
}

impl std::fmt::Debug for TokenCipher {
    /// Never prints the key.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("TokenCipher(<key withheld>)")
    }
}

impl TokenCipher {
    /// Read the key from the environment.
    ///
    /// Fails loudly when it is missing or malformed. The message says how to
    /// make one, because the alternative is somebody inventing a short key and
    /// having it padded silently.
    pub fn from_env() -> Result<Self, Error> {
        let raw = std::env::var(KEY_VAR).map_err(|_| {
            Error::Config(format!(
                "{KEY_VAR} is not set. The Google refresh token is encrypted at \
                 rest and cannot be stored without it. Generate one with: \
                 openssl rand -hex 32"
            ))
        })?;
        Self::from_hex(raw.trim())
    }

    pub fn from_hex(hex: &str) -> Result<Self, Error> {
        let bytes = decode_hex(hex).ok_or_else(|| {
            Error::Config(format!(
                "{KEY_VAR} is not valid hexadecimal. Generate one with: \
                 openssl rand -hex 32"
            ))
        })?;

        if bytes.len() != KEY_BYTES {
            return Err(Error::Config(format!(
                "{KEY_VAR} decodes to {} bytes; it must be exactly {KEY_BYTES}. \
                 Generate one with: openssl rand -hex 32",
                bytes.len()
            )));
        }

        Ok(Self {
            cipher: ChaCha20Poly1305::new(Key::from_slice(&bytes)),
        })
    }

    /// Encrypt a token for storage.
    ///
    /// A fresh random nonce every time, stored beside the ciphertext. Reusing a
    /// nonce with one key is the one mistake that breaks this cipher outright,
    /// and the only way to be sure it never happens is never to derive one.
    pub fn encrypt(&self, plaintext: &str) -> Result<String, Error> {
        let nonce = ChaCha20Poly1305::generate_nonce(&mut OsRng);
        let ciphertext = self
            .cipher
            .encrypt(&nonce, plaintext.as_bytes())
            .map_err(|_| Error::Internal("could not encrypt the refresh token".into()))?;

        Ok(format!(
            "{VERSION}:{}:{}",
            encode_hex(&nonce),
            encode_hex(&ciphertext)
        ))
    }

    /// Decrypt a stored token.
    ///
    /// A failure here is not "the token is wrong" — it is the wrong key, or a
    /// database somebody has edited. Both are worth saying out loud, because the
    /// remedy (reconnect) is the same but the cause is not.
    pub fn decrypt(&self, stored: &str) -> Result<String, Error> {
        let mut parts = stored.splitn(3, ':');
        let version = parts.next().unwrap_or_default();
        let nonce = parts.next().unwrap_or_default();
        let ciphertext = parts.next().unwrap_or_default();

        if version != VERSION {
            return Err(Error::Config(format!(
                "the stored refresh token is in format {version:?}, which this \
                 build does not read. Reconnect the Google account."
            )));
        }

        let nonce = decode_hex(nonce)
            .ok_or_else(|| Error::Config("the stored refresh token is malformed".into()))?;
        let ciphertext = decode_hex(ciphertext)
            .ok_or_else(|| Error::Config("the stored refresh token is malformed".into()))?;

        if nonce.len() != 12 {
            return Err(Error::Config(
                "the stored refresh token has the wrong nonce length".into(),
            ));
        }

        let plaintext = self
            .cipher
            .decrypt(Nonce::from_slice(&nonce), ciphertext.as_ref())
            .map_err(|_| {
                Error::Config(format!(
                    "the stored refresh token could not be decrypted. Either \
                     {KEY_VAR} has changed, or the database has been altered. \
                     Reconnect the Google account."
                ))
            })?;

        String::from_utf8(plaintext)
            .map_err(|_| Error::Internal("the decrypted refresh token is not text".into()))
    }
}

fn encode_hex(bytes: &[u8]) -> String {
    use std::fmt::Write;
    bytes.iter().fold(String::new(), |mut s, b| {
        let _ = write!(s, "{b:02x}");
        s
    })
}

fn decode_hex(hex: &str) -> Option<Vec<u8>> {
    if hex.len() % 2 != 0 || hex.is_empty() {
        return None;
    }
    (0..hex.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&hex[i..i + 2], 16).ok())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cipher() -> TokenCipher {
        TokenCipher::from_hex(&"ab".repeat(32)).unwrap()
    }

    #[test]
    fn a_token_survives_a_round_trip() {
        let c = cipher();
        let stored = c.encrypt("1//0abcdefgRefreshToken").unwrap();
        assert_eq!(c.decrypt(&stored).unwrap(), "1//0abcdefgRefreshToken");
    }

    #[test]
    fn the_stored_form_does_not_contain_the_token() {
        // The whole point. A stored value that happened to embed the plaintext
        // would pass a round-trip test and fail the requirement.
        let c = cipher();
        let stored = c.encrypt("1//0abcdefgRefreshToken").unwrap();
        assert!(!stored.contains("RefreshToken"), "{stored}");
        assert!(!stored.contains("1//0abcdefg"), "{stored}");
    }

    #[test]
    fn the_same_token_encrypts_differently_every_time() {
        // A fresh nonce per encryption. Identical ciphertexts would tell anyone
        // reading the database that two accounts share a token.
        let c = cipher();
        let first = c.encrypt("same token").unwrap();
        let second = c.encrypt("same token").unwrap();

        assert_ne!(first, second);
        assert_eq!(c.decrypt(&first).unwrap(), c.decrypt(&second).unwrap());
    }

    #[test]
    fn another_key_cannot_read_it() {
        let stored = cipher().encrypt("1//0abcdefgRefreshToken").unwrap();
        let other = TokenCipher::from_hex(&"cd".repeat(32)).unwrap();

        let err = other.decrypt(&stored).unwrap_err();
        assert!(err.to_string().contains("Reconnect"), "got {err}");
    }

    #[test]
    fn a_tampered_ciphertext_is_refused_rather_than_decrypted() {
        // Poly1305 authenticates as well as encrypts, so an edited database is
        // detected rather than yielding plausible rubbish.
        let c = cipher();
        let stored = c.encrypt("1//0abcdefgRefreshToken").unwrap();

        let mut parts: Vec<&str> = stored.split(':').collect();

        // Flip the first hex digit to a *different* one, whatever it is. The
        // previous form replaced the first 'a' with 'b', which tampered with
        // nothing at all when the ciphertext happened to contain no 'a' — and
        // the nonce is fresh every run, so that was roughly one run in
        // thirteen where this test passed a ciphertext it had not edited.
        let (head, tail) = parts[2].split_at(1);
        let flipped = format!("{}{tail}", if head == "0" { "1" } else { "0" });
        assert_ne!(flipped, parts[2], "the ciphertext must actually be edited");
        parts[2] = &flipped;

        assert!(c.decrypt(&parts.join(":")).is_err());
    }

    #[test]
    fn a_key_of_the_wrong_length_is_refused_not_padded() {
        let err = TokenCipher::from_hex("abcd").unwrap_err();
        assert!(err.to_string().contains("exactly 32"), "got {err}");
    }

    #[test]
    fn a_key_that_is_not_hexadecimal_is_refused() {
        let err = TokenCipher::from_hex(&"zz".repeat(32)).unwrap_err();
        assert!(err.to_string().contains("hexadecimal"), "got {err}");
    }

    #[test]
    fn a_missing_key_names_the_variable_and_how_to_make_one() {
        // The failure a person actually meets on first deployment.
        std::env::remove_var(KEY_VAR);
        let err = TokenCipher::from_env().unwrap_err();

        assert!(err.to_string().contains(KEY_VAR));
        assert!(
            err.to_string().contains("openssl rand -hex 32"),
            "got {err}"
        );
    }

    #[test]
    fn a_stored_value_from_an_unknown_format_says_so() {
        let err = cipher().decrypt("v9:aabb:ccdd").unwrap_err();
        assert!(err.to_string().contains("does not read"), "got {err}");
    }

    #[test]
    fn the_key_is_never_printed() {
        let debug = format!("{:?}", cipher());
        assert!(!debug.contains("abab"), "{debug}");
        assert!(debug.contains("withheld"), "{debug}");
    }
}
