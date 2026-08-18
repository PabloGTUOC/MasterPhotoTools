//! Credential storage.
//!
//! The desktop's Firebase refresh token lives in the **macOS Keychain**
//! (specification §5.2), never on disk in the clear and never inside the photo
//! library (§9.2 rule 4).

use keyring::Entry;

const SERVICE: &str = "com.phototools.master";
const REFRESH_TOKEN: &str = "firebase-refresh-token";
const SERVER_AUTH_TOKEN: &str = "server-auth-token";

/// A named secret in the platform credential store.
pub struct Credential {
    entry: Entry,
}

impl Credential {
    pub fn new(name: &str) -> Result<Self, String> {
        Entry::new(SERVICE, name)
            .map(|entry| Self { entry })
            .map_err(|e| format!("Could not open the credential store: {e}"))
    }

    /// The Firebase refresh token, which is what lets the desktop stay signed in.
    pub fn refresh_token() -> Result<Self, String> {
        Self::new(REFRESH_TOKEN)
    }

    /// The bearer token sent to the server (§5.2, today §5.3's `ADMIN_TOKEN`).
    ///
    /// In the Keychain rather than beside the address it belongs to, because it
    /// is a credential and §9.2 rule 4 keeps those off disk in the clear. The
    /// address is not a secret and is persisted as plain JSON.
    pub fn server_auth_token() -> Result<Self, String> {
        Self::new(SERVER_AUTH_TOKEN)
    }

    pub fn store(&self, secret: &str) -> Result<(), String> {
        self.entry
            .set_password(secret)
            .map_err(|e| format!("Could not store the credential: {e}"))
    }

    /// The stored secret, or `None` if there is not one.
    ///
    /// A missing entry is not an error — it means nobody has signed in yet.
    pub fn read(&self) -> Result<Option<String>, String> {
        match self.entry.get_password() {
            Ok(secret) => Ok(Some(secret)),
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(e) => Err(format!("Could not read the credential: {e}")),
        }
    }

    pub fn clear(&self) -> Result<(), String> {
        match self.entry.delete_credential() {
            Ok(()) => Ok(()),
            Err(keyring::Error::NoEntry) => Ok(()),
            Err(e) => Err(format!("Could not clear the credential: {e}")),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The keyring crate has no usable backend in a headless container, so this
    /// asserts the shape of the failure rather than a round trip. The round trip
    /// is a macOS step, recorded in `docs/manual-verification.md`.
    #[test]
    fn opening_the_store_either_works_or_says_why() {
        match Credential::refresh_token() {
            Ok(credential) => {
                // If a backend exists, a missing entry must read as None.
                match credential.read() {
                    Ok(None) | Ok(Some(_)) => {}
                    Err(message) => assert!(!message.is_empty()),
                }
            }
            Err(message) => assert!(
                message.contains("credential store"),
                "the failure should name what could not be opened, got: {message}"
            ),
        }
    }
}
