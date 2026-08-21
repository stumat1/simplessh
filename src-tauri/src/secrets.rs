//! Stores saved passwords in the OS keystore via the keyring crate (Windows
//! Credential Manager on Windows). Plaintext never crosses to the renderer —
//! only presence checks and forget; the decrypted value is injected into
//! connect requests backend-side.

const SERVICE: &str = "simplerssh";

/// Stable id for a stored password: pw:user@host:port.
pub fn password_id(host: &str, port: u16, username: &str) -> String {
    format!("pw:{username}@{host}:{port}")
}

fn entry(id: &str) -> Result<keyring::Entry, keyring::Error> {
    keyring::Entry::new(SERVICE, id)
}

pub struct SecretStore;

impl SecretStore {
    pub fn has(&self, id: &str) -> bool {
        entry(id).and_then(|e| e.get_password()).is_ok()
    }

    /// Persist a secret. Returns false if the OS keystore rejected it.
    pub fn set(&self, id: &str, plaintext: &str) -> bool {
        entry(id).and_then(|e| e.set_password(plaintext)).is_ok()
    }

    /// Fetch a stored secret, or None if absent/unavailable.
    pub fn get(&self, id: &str) -> Option<String> {
        entry(id).and_then(|e| e.get_password()).ok()
    }

    pub fn delete(&self, id: &str) {
        let _ = entry(id).and_then(|e| e.delete_credential());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn password_id_scheme() {
        assert_eq!(password_id("example.com", 22, "alice"), "pw:alice@example.com:22");
    }
}
