//! Serde mirrors of the renderer's domain types (src/shared/types.ts).
//! Field names must serialize exactly as the TypeScript shapes (camelCase).

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HostEntry {
    pub id: String,
    pub name: String,
    pub host: String,
    pub port: u16,
    pub username: String,
    /// Persist the password (encrypted) on a successful connect.
    pub save_password: bool,
}

impl HostEntry {
    /// Whether both entries point at the same user@host:port — the identity a
    /// saved password is keyed by.
    pub fn same_target(&self, other: &HostEntry) -> bool {
        self.host == other.host && self.port == other.port && self.username == other.username
    }
}

/// A transient request to open a connection (not necessarily a saved host).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConnectRequest {
    pub host: String,
    pub port: u16,
    pub username: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub password: Option<String>,
    /// Persist the password (encrypted) after a successful connection.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub save_password: Option<bool>,
}

/// Sent to the renderer when a host key needs the user's trust decision.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HostKeyPrompt {
    pub session_id: String,
    pub host: String,
    pub port: u16,
    /// SHA256 fingerprint of the presented key.
    pub fingerprint: String,
    /// 'unknown' = never seen; 'changed' = differs from a previously trusted key.
    pub status: &'static str,
    /// For 'changed': the fingerprint we had on file.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub known_fingerprint: Option<String>,
    /// Set when the known-hosts file couldn't be loaded, so keys trusted
    /// earlier can't be checked and every host looks unknown.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub store_warning: Option<String>,
}

/// Initial PTY dimensions sent with a connect request.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct TerminalSize {
    pub cols: u32,
    pub rows: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum SessionStatus {
    Connecting,
    Ready,
    Closed,
    Error,
}
