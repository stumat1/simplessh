//! Persistent known-hosts store keyed by `host:port`, backed by a JSON file.
//! Same schema as the Electron build's known_hosts.json. Compares the full key
//! (not just the fingerprint) to decide match/changed.

use std::collections::HashMap;
use std::path::PathBuf;

use base64::engine::general_purpose::{STANDARD, STANDARD_NO_PAD};
use base64::Engine;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

/// OpenSSH-style SHA256 fingerprint (base64, no padding) of a raw host key.
pub fn fingerprint_of(key: &[u8]) -> String {
    STANDARD_NO_PAD.encode(Sha256::digest(key))
}

#[derive(Debug, PartialEq, Eq)]
pub enum HostKeyCheck {
    Match,
    Unknown {
        fingerprint: String,
    },
    Changed {
        fingerprint: String,
        known_fingerprint: String,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct StoredEntry {
    /// Base64 of the raw host key (authoritative for comparison).
    key: String,
    /// Cached fingerprint for display.
    fingerprint: String,
}

pub struct KnownHostsStore {
    file_path: PathBuf,
    entries: HashMap<String, StoredEntry>,
}

impl KnownHostsStore {
    pub fn new(file_path: PathBuf) -> Self {
        // Corrupt/unreadable store — start empty rather than crash.
        let entries = std::fs::read_to_string(&file_path)
            .ok()
            .and_then(|text| serde_json::from_str(&text).ok())
            .unwrap_or_default();
        Self { file_path, entries }
    }

    fn id(host: &str, port: u16) -> String {
        format!("{host}:{port}")
    }

    fn save(&self) -> Result<(), String> {
        if let Some(dir) = self.file_path.parent() {
            std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
        }
        let json = serde_json::to_string_pretty(&self.entries).map_err(|e| e.to_string())?;
        std::fs::write(&self.file_path, json).map_err(|e| e.to_string())
    }

    /// Classify a presented host key against what we have stored.
    pub fn check(&self, host: &str, port: u16, key: &[u8]) -> HostKeyCheck {
        let fingerprint = fingerprint_of(key);
        match self.entries.get(&Self::id(host, port)) {
            None => HostKeyCheck::Unknown { fingerprint },
            Some(existing) if existing.key == STANDARD.encode(key) => HostKeyCheck::Match,
            Some(existing) => HostKeyCheck::Changed {
                fingerprint,
                known_fingerprint: existing.fingerprint.clone(),
            },
        }
    }

    /// Persist (or replace) the trusted key for a host. A write failure is
    /// logged, not surfaced mid-handshake: the connection still proceeds
    /// (the user already made the trust decision) and worst case the host
    /// is re-prompted next time.
    pub fn trust(&mut self, host: &str, port: u16, key: &[u8]) {
        self.entries.insert(
            Self::id(host, port),
            StoredEntry {
                key: STANDARD.encode(key),
                fingerprint: fingerprint_of(key),
            },
        );
        if let Err(e) = self.save() {
            log::warn!("failed to persist known_hosts for {host}:{port}: {e}");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fingerprint_matches_openssh_format() {
        // SHA256 of empty input, base64 without padding.
        assert_eq!(
            fingerprint_of(b""),
            "47DEQpj8HBSa+/TImW+5JCeuQeRkm5NMpJWZG3hSuFU"
        );
    }

    #[test]
    fn check_and_trust_roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("known_hosts.json");
        let mut store = KnownHostsStore::new(path.clone());

        let key = b"some-raw-host-key";
        assert!(matches!(
            store.check("example.com", 22, key),
            HostKeyCheck::Unknown { .. }
        ));

        store.trust("example.com", 22, key);
        assert_eq!(store.check("example.com", 22, key), HostKeyCheck::Match);

        // A different key for the same host reports 'changed' with the old fingerprint.
        let check = store.check("example.com", 22, b"different-key");
        match check {
            HostKeyCheck::Changed {
                known_fingerprint, ..
            } => assert_eq!(known_fingerprint, fingerprint_of(key)),
            other => panic!("expected Changed, got {other:?}"),
        }

        // Reload from disk — trust persists.
        let reloaded = KnownHostsStore::new(path);
        assert_eq!(reloaded.check("example.com", 22, key), HostKeyCheck::Match);
    }
}
