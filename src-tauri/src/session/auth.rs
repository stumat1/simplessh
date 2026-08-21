//! Connection + authentication: password auth only, with a silent fallback
//! for servers that route plain password auth through keyboard-interactive
//! (common with PAM-based OpenSSH). If a server asks for more than a single
//! hidden prompt (real MFA/OTP), the connection fails with a clear error —
//! there is no UI for answering multi-round challenges.

use std::sync::Arc;

use russh::client::{AuthResult, Handle, KeyboardInteractiveAuthResponse};
use tauri::{AppHandle, Manager};

use super::handler::ClientHandler;
use crate::error::{Error, Result};
use crate::state::AppState;
use crate::types::ConnectRequest;

/// How long the banner/kex phase may make no progress before we give up on a
/// server that accepted TCP but stalled. The host-key prompt (which pauses
/// the handshake on the user) extends the deadline — see connect_and_auth.
const HANDSHAKE_STALL_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(30);

pub async fn connect_and_auth(
    app: &AppHandle,
    session_id: &str,
    req: &ConnectRequest,
    config: Arc<russh::client::Config>,
) -> Result<Handle<ClientHandler>> {
    let handler = ClientHandler {
        app: app.clone(),
        session_id: session_id.to_string(),
        host: req.host.clone(),
        port: req.port,
    };

    // Bound only the TCP connect: everything after it can block on user
    // prompts (host-key trust) with their own timeouts.
    let stream = tokio::time::timeout(
        std::time::Duration::from_secs(20),
        tokio::net::TcpStream::connect((req.host.as_str(), req.port)),
    )
    .await
    .map_err(|_| Error::msg("Timed out while connecting"))?
    .map_err(|e| Error::msg(format!("Cannot connect to {}:{}: {e}", req.host, req.port)))?;

    // Bound the handshake (banner + kex): a server that goes silent here must
    // not pin the tab on "Connecting…" forever. The host-key trust prompt runs
    // *inside* this future and may legitimately sit on the user for minutes,
    // so when the deadline fires while a prompt is pending we extend it past
    // the prompt's own timeout instead of aborting.
    let handshake = russh::client::connect_stream(config, stream, handler);
    tokio::pin!(handshake);
    let mut deadline = tokio::time::Instant::now() + HANDSHAKE_STALL_TIMEOUT;
    let mut handle = loop {
        tokio::select! {
            result = &mut handshake => {
                break result.map_err(|e| Error::msg(e.to_string()))?;
            }
            _ = tokio::time::sleep_until(deadline) => {
                let prompt_pending = {
                    let state = app.state::<AppState>();
                    let pending = state.pending.host_key.lock().unwrap();
                    pending.contains_key(session_id)
                };
                if !prompt_pending {
                    return Err(Error::msg("The server did not complete the SSH handshake in time"));
                }
                deadline += super::handler::PROMPT_TIMEOUT;
            }
        }
    };

    let password = req.password.as_deref().unwrap_or("");
    if auth_password(&mut handle, &req.username, password).await? {
        return Ok(handle);
    }
    if keyboard_interactive_password(&mut handle, &req.username, password).await? {
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
) -> Result<bool> {
    let result = handle
        .authenticate_password(username, password)
        .await
        .map_err(|e| Error::msg(e.to_string()))?;
    Ok(matches!(result, AuthResult::Success))
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
