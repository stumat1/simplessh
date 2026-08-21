//! russh client handler: host-key trust policy. Silent on a known match,
//! otherwise asks the renderer (ssh:hostkey-prompt event) and awaits the
//! decision command, persisting the key on accept. Port of verifyHostKey in
//! src/main/index.ts, but runs *inside* the handshake via the async trait.

use std::time::Duration;

use russh::client;
use russh::keys::PublicKey;
use tauri::{AppHandle, Emitter, Manager};
use tokio::sync::oneshot;

use crate::known_hosts::HostKeyCheck;
use crate::state::AppState;
use crate::types::HostKeyPrompt;

/// How long an auth prompt (host-key trust here, keyboard-interactive in
/// auth.rs) may sit unanswered before we abort the attempt.
pub const PROMPT_TIMEOUT: Duration = Duration::from_secs(300);

pub struct ClientHandler {
    pub app: AppHandle,
    pub session_id: String,
    pub host: String,
    pub port: u16,
}

impl client::Handler for ClientHandler {
    type Error = russh::Error;

    async fn check_server_key(
        &mut self,
        server_public_key: &PublicKey,
    ) -> Result<bool, Self::Error> {
        // Compare the raw wire-format key blob — the same bytes ssh2 handed to
        // hostVerifier, so entries migrated from the Electron build still match.
        let Ok(key_bytes) = server_public_key.to_bytes() else {
            return Ok(false); // un-encodable key — fail closed
        };

        let state = self.app.state::<AppState>();

        // Scope the lock: it must not be held across the await below.
        let prompt = {
            let known_hosts = state.known_hosts.lock().unwrap();
            match known_hosts.check(&self.host, self.port, &key_bytes) {
                HostKeyCheck::Match => return Ok(true),
                HostKeyCheck::Unknown { fingerprint } => HostKeyPrompt {
                    session_id: self.session_id.clone(),
                    host: self.host.clone(),
                    port: self.port,
                    fingerprint,
                    status: "unknown",
                    known_fingerprint: None,
                },
                HostKeyCheck::Changed {
                    fingerprint,
                    known_fingerprint,
                } => HostKeyPrompt {
                    session_id: self.session_id.clone(),
                    host: self.host.clone(),
                    port: self.port,
                    fingerprint,
                    status: "changed",
                    known_fingerprint: Some(known_fingerprint),
                },
            }
        };

        let (tx, rx) = oneshot::channel();
        state
            .pending
            .host_key
            .lock()
            .unwrap()
            .insert(self.session_id.clone(), tx);
        let _ = self.app.emit("ssh:hostkey-prompt", prompt);

        let accepted = match tokio::time::timeout(PROMPT_TIMEOUT, rx).await {
            Ok(Ok(accepted)) => accepted,
            // Timeout or the sender was dropped (session torn down) — reject.
            _ => false,
        };
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
