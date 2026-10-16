//! Connection + authentication: password auth only, with a silent fallback
//! for servers that route plain password auth through keyboard-interactive
//! (common with PAM-based OpenSSH). If a server asks for more than a single
//! hidden prompt (real MFA/OTP), the connection fails with a clear error —
//! there is no UI for answering multi-round challenges.

use std::sync::Arc;

use russh::client::{AuthResult, Handle, KeyboardInteractiveAuthResponse};
use russh::MethodKind;
use tauri::AppHandle;
use tokio::time::Instant;

use super::handler::ClientHandler;
use crate::error::{Error, Result};
use crate::types::ConnectRequest;

/// How long the banner/kex phase may make no progress before we give up on a
/// server that accepted TCP but stalled. The clock is paused while the
/// host-key prompt waits on the user — see connect_and_auth.
const HANDSHAKE_STALL_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(30);
/// Bound password and keyboard-interactive exchanges after host-key approval.
const AUTH_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(45);

pub async fn connect_and_auth(
    app: &AppHandle,
    session_id: &str,
    req: &ConnectRequest,
    config: Arc<russh::client::Config>,
) -> Result<Handle<ClientHandler>> {
    let (prompt_tx, mut prompt_rx) = tokio::sync::watch::channel(false);
    let handler = ClientHandler {
        app: app.clone(),
        session_id: session_id.to_string(),
        host: req.host.clone(),
        port: req.port,
        prompt_open: prompt_tx,
    };

    // Bound the TCP connect. The handshake and auth phases below carry their
    // own deadlines (the host-key prompt extends the handshake's).
    let stream = tokio::time::timeout(
        std::time::Duration::from_secs(20),
        tokio::net::TcpStream::connect((req.host.as_str(), req.port)),
    )
    .await
    .map_err(|_| Error::msg("Timed out while connecting"))?
    .map_err(|e| Error::msg(format!("Cannot connect to {}:{}: {e}", req.host, req.port)))?;

    // Bound the handshake (banner + kex): a server that goes silent here must
    // not pin the tab on "Connecting…" forever. The host-key trust prompt runs
    // *inside* this future and may legitimately sit on the user for minutes
    // (it has its own timeout), so the stall clock stops while the prompt is
    // open and restarts from zero once it is answered.
    let handshake = russh::client::connect_stream(config, stream, handler);
    tokio::pin!(handshake);
    let mut deadline = Instant::now() + HANDSHAKE_STALL_TIMEOUT;
    let mut prompt_open = false;
    let mut watch_prompt = true;
    let mut handle = loop {
        tokio::select! {
            result = &mut handshake => {
                break result.map_err(|e| Error::msg(e.to_string()))?;
            }
            changed = prompt_rx.changed(), if watch_prompt => {
                if changed.is_err() {
                    // Handler dropped: the handshake is finishing anyway.
                    watch_prompt = false;
                } else {
                    prompt_open = *prompt_rx.borrow_and_update();
                    if !prompt_open {
                        deadline = Instant::now() + HANDSHAKE_STALL_TIMEOUT;
                    }
                }
            }
            _ = tokio::time::sleep_until(deadline), if !prompt_open => {
                return Err(Error::msg("The server did not complete the SSH handshake in time"));
            }
        }
    };

    let auth = async {
        let password = req.password.as_deref().unwrap_or("");
        match auth_password(&mut handle, &req.username, password).await? {
            AuthResult::Success => return Ok::<bool, Error>(true),
            // Password accepted but the server wants a second factor.
            AuthResult::Failure {
                partial_success: true,
                ..
            } => return Ok(false),
            // Only retry via keyboard-interactive when the server offers it,
            // so a wrong password costs one failed attempt (fail2ban,
            // lockout policies), not two.
            AuthResult::Failure {
                remaining_methods, ..
            } if remaining_methods.contains(&MethodKind::KeyboardInteractive) => {}
            AuthResult::Failure { .. } => return Ok(false),
        }
        keyboard_interactive_password(&mut handle, &req.username, password).await
    };
    let authenticated = tokio::time::timeout(AUTH_TIMEOUT, auth)
        .await
        .map_err(|_| Error::msg("Timed out while authenticating"))??;
    if authenticated {
        return Ok(handle);
    }

    Err(Error::msg(
        "Authentication failed. If this server requires multi-factor auth, that isn't supported.",
    ))
}

async fn auth_password(
    handle: &mut Handle<ClientHandler>,
    username: &str,
    password: &str,
) -> Result<AuthResult> {
    handle
        .authenticate_password(username, password)
        .await
        .map_err(|e| Error::msg(e.to_string()))
}

/// Many OpenSSH servers implement password auth *via* keyboard-interactive
/// (PAM). If the server presents exactly one hidden prompt, answer it with
/// the typed password automatically. Anything else (multiple prompts, a
/// visible/OTP prompt) is treated as unsupported MFA and fails the round.
async fn keyboard_interactive_password(
    handle: &mut Handle<ClientHandler>,
    username: &str,
    password: &str,
) -> Result<bool> {
    let mut response = handle
        .authenticate_keyboard_interactive_start(username, None)
        .await
        .map_err(|e| Error::msg(e.to_string()))?;

    let mut auto_used = false;
    loop {
        match response {
            KeyboardInteractiveAuthResponse::Success => return Ok(true),
            KeyboardInteractiveAuthResponse::Failure { .. } => return Ok(false),
            KeyboardInteractiveAuthResponse::InfoRequest { prompts, .. } => {
                let answers = if !auto_used && prompts.len() == 1 && !prompts[0].echo {
                    auto_used = true;
                    vec![password.to_string()]
                } else if prompts.is_empty() {
                    // Informational round — respond with no answers to continue.
                    Vec::new()
                } else {
                    // A second round, or a visible-echo prompt: real MFA/OTP.
                    return Ok(false);
                };
                response = handle
                    .authenticate_keyboard_interactive_respond(answers)
                    .await
                    .map_err(|e| Error::msg(e.to_string()))?;
            }
        }
    }
}
