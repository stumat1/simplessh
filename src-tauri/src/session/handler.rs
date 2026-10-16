//! russh client handler: host-key trust policy. Silent on a known match,
//! otherwise asks the renderer (ssh:hostkey-prompt event) and awaits the
//! decision command, persisting the key on accept. Runs *inside* the
//! handshake via the async trait.

use std::time::Duration;

use russh::client;
use russh::keys::PublicKeyOrCertificate;
use tauri::{AppHandle, Emitter, Manager};
use tokio::sync::{oneshot, watch};

use crate::known_hosts::HostKeyCheck;
use crate::state::AppState;
use crate::types::HostKeyPrompt;

/// How long the host-key trust prompt may sit unanswered before the key is
/// rejected.
const PROMPT_TIMEOUT: Duration = Duration::from_secs(300);

pub struct ClientHandler {
    pub app: AppHandle,
    pub session_id: String,
    pub host: String,
    pub port: u16,
    /// True while the trust prompt waits on the user, so the handshake's
    /// stall timeout (auth.rs) doesn't count that time.
    pub prompt_open: watch::Sender<bool>,
}

impl client::Handler for ClientHandler {
    type Error = russh::Error;

    async fn check_server_key(
        &mut self,
        server_public_key: &PublicKeyOrCertificate,
    ) -> Result<bool, Self::Error> {
        // Compare the raw wire-format key blob — the same bytes ssh2 handed to
        // hostVerifier, so entries migrated from the Electron build still match.
        // A host certificate is fingerprinted by its underlying public key,
        // since this app has no certificate-specific trust UI.
        let Ok(key_bytes) = server_public_key.public_key().to_bytes() else {
            return Ok(false); // un-encodable key — fail closed
        };

        let state = self.app.state::<AppState>();

        // Scope the lock: it must not be held across the await below.
        let prompt = {
            let known_hosts = state.known_hosts.lock().unwrap();
            let (fingerprint, status, known_fingerprint) =
                match known_hosts.check(&self.host, self.port, &key_bytes) {
                    HostKeyCheck::Match => return Ok(true),
                    HostKeyCheck::Unknown { fingerprint } => (fingerprint, "unknown", None),
                    HostKeyCheck::Changed {
                        fingerprint,
                        known_fingerprint,
                    } => (fingerprint, "changed", Some(known_fingerprint)),
                };
            HostKeyPrompt {
                session_id: self.session_id.clone(),
                host: self.host.clone(),
                port: self.port,
                fingerprint,
                status,
                known_fingerprint,
                store_warning: known_hosts.load_warning().map(str::to_owned),
            }
        };

        let (tx, rx) = oneshot::channel();
        state
            .pending
            .host_key
            .lock()
            .unwrap()
            .insert(self.session_id.clone(), tx);
        self.prompt_open.send_replace(true);
        let _ = self.app.emit("ssh:hostkey-prompt", prompt);

        let accepted = match tokio::time::timeout(PROMPT_TIMEOUT, rx).await {
            Ok(Ok(accepted)) => accepted,
            // Timeout or the sender was dropped (session torn down) — reject.
            _ => false,
        };
        self.prompt_open.send_replace(false);
        state
            .pending
            .host_key
            .lock()
            .unwrap()
            .remove(&self.session_id);

        if accepted {
            state
                .known_hosts
                .lock()
                .unwrap()
                .trust(&self.host, self.port, &key_bytes);
        }
        Ok(accepted)
    }
}
