//! Managed application state: the stores plus the pending host-key-trust
//! decisions that bridge async renderer round-trips back into a paused SSH
//! handshake.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Mutex;

use tokio::sync::oneshot;

use crate::hosts::HostStore;
use crate::known_hosts::KnownHostsStore;
use crate::secrets::SecretStore;

/// Secret to persist iff the connection authenticates (reaches 'ready').
pub struct PendingSecretSave {
    pub id: String,
    pub value: String,
}

#[derive(Default)]
pub struct PendingPrompts {
    /// Host-key trust decisions awaiting a renderer response, by sessionId.
    pub host_key: Mutex<HashMap<String, oneshot::Sender<bool>>>,
}

impl PendingPrompts {
    /// Abort any prompt still waiting for this session (deny).
    pub fn drain_session(&self, session_id: &str) {
        if let Some(tx) = self.host_key.lock().unwrap().remove(session_id) {
            let _ = tx.send(false);
        }
    }
}

pub struct AppState {
    pub known_hosts: Mutex<KnownHostsStore>,
    pub hosts: Mutex<HostStore>,
    pub secrets: SecretStore,
    pub pending: PendingPrompts,
    /// Secrets to persist on successful auth, by sessionId.
    pub pending_secret_saves: Mutex<HashMap<String, PendingSecretSave>>,
    pub sessions: crate::session::SessionManager,
}

impl AppState {
    pub fn new(data_dir: PathBuf) -> Self {
        Self {
            known_hosts: Mutex::new(KnownHostsStore::new(data_dir.join("known_hosts.json"))),
            hosts: Mutex::new(HostStore::new(data_dir.join("hosts.json"))),
            secrets: SecretStore,
            pending: PendingPrompts::default(),
            pending_secret_saves: Mutex::new(HashMap::new()),
            sessions: crate::session::SessionManager::new(),
        }
    }
}
