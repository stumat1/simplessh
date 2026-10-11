//! Owns all live russh connections and their shell channels, keyed by
//! sessionId. The single place with network access.

mod auth;
mod handler;

use std::collections::HashMap;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use tauri::ipc::{Channel, InvokeResponseBody};
use tauri::{AppHandle, Emitter, Manager};
use tokio::sync::mpsc;
use uuid::Uuid;

use crate::state::AppState;
use crate::types::{ConnectRequest, SessionStatus, TerminalSize};

/// Commands the rest of the app can send into a live session task.
enum SessionCmd {
    Write(Vec<u8>),
    Resize { cols: u32, rows: u32 },
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum SessionSignal {
    None,
    Disconnect,
    InputOverflow,
}

/// Input is bounded by both message count (keystrokes are one message each)
/// and total queued bytes (a paste arrives as one large message). Either
/// limit is only reached when the remote has stopped accepting input.
const COMMAND_QUEUE_CAPACITY: usize = 1024;
const MAX_QUEUED_INPUT_BYTES: usize = 8 * 1024 * 1024;

struct SessionHandle {
    cmd_tx: mpsc::Sender<SessionCmd>,
    signal_tx: tokio::sync::watch::Sender<SessionSignal>,
    /// Bytes accepted by `write` but not yet handed to the SSH channel.
    queued_input: Arc<AtomicUsize>,
}

/// The session task's ends of the `SessionHandle` channels.
struct SessionInbox {
    cmd_rx: mpsc::Receiver<SessionCmd>,
    signal_rx: tokio::sync::watch::Receiver<SessionSignal>,
    queued_input: Arc<AtomicUsize>,
}

#[derive(Default)]
pub struct SessionManager {
    sessions: Mutex<HashMap<String, SessionHandle>>,
}

/// Event payloads for the low-frequency renderer events. Terminal output does
/// not go through events — it streams over the per-session IPC `Channel`.
#[derive(serde::Serialize, Clone)]
#[serde(rename_all = "camelCase")]
struct StatusEvent {
    session_id: String,
    status: SessionStatus,
}

#[derive(serde::Serialize, Clone)]
#[serde(rename_all = "camelCase")]
struct ErrorEvent {
    session_id: String,
    message: String,
}

pub fn emit_status(app: &AppHandle, session_id: &str, status: SessionStatus) {
    let _ = app.emit(
        "ssh:status",
        StatusEvent {
            session_id: session_id.to_string(),
            status,
        },
    );
}

fn emit_error(app: &AppHandle, session_id: &str, message: String) {
    let _ = app.emit(
        "ssh:error",
        ErrorEvent {
            session_id: session_id.to_string(),
            message,
        },
    );
}

impl SessionManager {
    pub fn new() -> Self {
        Self::default()
    }

    /// Number of live sessions — used to assert no leaks after disconnects.
    pub fn size(&self) -> usize {
        self.sessions.lock().unwrap().len()
    }

    /// Opens an SSH connection and a PTY shell. Returns the new sessionId
    /// immediately; connection progress is reported via status/error events
    /// and the data channel.
    pub fn connect(
        &self,
        app: AppHandle,
        req: ConnectRequest,
        size: Option<TerminalSize>,
        on_data: Channel<InvokeResponseBody>,
    ) -> String {
        let session_id = Uuid::new_v4().to_string();
        let (cmd_tx, cmd_rx) = mpsc::channel(COMMAND_QUEUE_CAPACITY);
        let (signal_tx, signal_rx) = tokio::sync::watch::channel(SessionSignal::None);
        let queued_input = Arc::new(AtomicUsize::new(0));
        self.sessions.lock().unwrap().insert(
            session_id.clone(),
            SessionHandle {
                cmd_tx,
                signal_tx,
                queued_input: queued_input.clone(),
            },
        );
        let inbox = SessionInbox {
            cmd_rx,
            signal_rx,
            queued_input,
        };
        emit_status(&app, &session_id, SessionStatus::Connecting);

        let id = session_id.clone();
        tauri::async_runtime::spawn(async move {
            run_session(app, id, req, size, on_data, inbox).await;
        });

        session_id
    }

    /// Queues user input with a strict memory bound. Overload terminates the
    /// session so input is never silently lost. Kept synchronous so input
    /// reaches the queue in the order the renderer sent it.
    pub fn write(&self, session_id: &str, data: &str) {
        if let Some(s) = self.sessions.lock().unwrap().get(session_id) {
            let len = data.len();
            let queued = s.queued_input.fetch_add(len, Ordering::AcqRel) + len;
            if queued > MAX_QUEUED_INPUT_BYTES
                || s.cmd_tx
                    .try_send(SessionCmd::Write(data.as_bytes().to_vec()))
                    .is_err()
            {
                s.queued_input.fetch_sub(len, Ordering::AcqRel);
                // Close explicitly on overload rather than silently dropping
                // terminal input. This signal has its own channel so a full
                // command queue cannot prevent teardown. A disconnect already
                // requested takes precedence.
                s.signal_tx.send_if_modified(|signal| {
                    if *signal == SessionSignal::None {
                        *signal = SessionSignal::InputOverflow;
                        true
                    } else {
                        false
                    }
                });
            }
        }
    }

    /// Resizes the remote PTY to match the terminal grid.
    pub fn resize(&self, session_id: &str, cols: u32, rows: u32) {
        if let Some(s) = self.sessions.lock().unwrap().get(session_id) {
            let _ = s.cmd_tx.try_send(SessionCmd::Resize { cols, rows });
        }
    }

    /// Requests a graceful disconnect; final cleanup happens in the session task.
    pub fn disconnect(&self, session_id: &str) {
        if let Some(s) = self.sessions.lock().unwrap().get(session_id) {
            let _ = s.signal_tx.send(SessionSignal::Disconnect);
        }
    }

    /// Disconnects every live session (e.g. on app exit).
    pub fn disconnect_all(&self) {
        for s in self.sessions.lock().unwrap().values() {
            let _ = s.signal_tx.send(SessionSignal::Disconnect);
        }
    }

    /// Remove a session from the registry; returns false if already removed
    /// (so callers can avoid emitting duplicate terminal status events).
    fn take(&self, session_id: &str) -> bool {
        self.sessions.lock().unwrap().remove(session_id).is_some()
    }
}

/// Final teardown: deregister, drop pending state, emit a single terminal
/// status.
fn cleanup(app: &AppHandle, session_id: &str, status: SessionStatus) {
    let state = app.state::<AppState>();
    if !state.sessions.take(session_id) {
        return; // already cleaned up — avoids duplicate status events
    }
    state
        .pending_secret_saves
        .lock()
        .unwrap()
        .remove(session_id);
    state.pending.drain_session(session_id);
    emit_status(app, session_id, status);
    log::debug!("live sessions: {} ({:?})", state.sessions.size(), status);
}

fn fail(app: &AppHandle, session_id: &str, message: String) {
    if app
        .state::<AppState>()
        .sessions
        .sessions
        .lock()
        .unwrap()
        .contains_key(session_id)
    {
        emit_error(app, session_id, message);
        cleanup(app, session_id, SessionStatus::Error);
    }
}

/// Auth succeeded — persist the password (if requested).
fn on_ready(app: &AppHandle, session_id: &str) {
    let state = app.state::<AppState>();
    if let Some(pending) = state
        .pending_secret_saves
        .lock()
        .unwrap()
        .remove(session_id)
    {
        state.secrets.set(&pending.id, &pending.value);
    }
    emit_status(app, session_id, SessionStatus::Ready);
}

/// The per-session task: connect, authenticate, open a PTY shell, then pump
/// bytes both ways until the channel closes or a disconnect is requested.
async fn run_session(
    app: AppHandle,
    session_id: String,
    req: ConnectRequest,
    size: Option<TerminalSize>,
    on_data: Channel<InvokeResponseBody>,
    inbox: SessionInbox,
) {
    let SessionInbox {
        mut cmd_rx,
        mut signal_rx,
        queued_input,
    } = inbox;
    // Dead-peer detection: send a keepalive every 15s; after keepalive_max
    // (default 3) go unanswered, russh returns KeepaliveTimeout and the
    // session tears down into the reconnect overlay. inactivity_timeout is
    // deliberately left None: idle-but-healthy SSH sessions must not be dropped.
    let config = Arc::new(russh::client::Config {
        keepalive_interval: Some(Duration::from_secs(15)),
        ..Default::default()
    });

    // Host-key approval may wait on the user; the other network phases have
    // explicit deadlines. Keep processing resize and cancellation while the
    // connection future is pending.
    let (cols, rows) = size.map(|s| (s.cols, s.rows)).unwrap_or((80, 24));
    let connect = async {
        let handle = auth::connect_and_auth(&app, &session_id, &req, config)
            .await
            .map_err(|e| e.to_string())?;
        let channel =
            tokio::time::timeout(Duration::from_secs(30), open_shell(&handle, cols, rows))
                .await
                .map_err(|_| "Timed out while opening the SSH shell".to_string())?
                .map_err(|e| e.to_string())?;
        Ok::<_, String>((handle, channel))
    };
    tokio::pin!(connect);
    // The PTY doesn't exist yet, so remember the latest resize seen while
    // connecting and apply it once the shell is open.
    let mut queued_resize: Option<(u32, u32)> = None;
    let (handle, mut channel) = loop {
        tokio::select! {
            result = &mut connect => match result {
                Ok(pair) => break pair,
                Err(message) => {
                    fail(&app, &session_id, message);
                    return;
                }
            },
            cmd = cmd_rx.recv() => match cmd {
                None => {
                    // Dropping the connect future tears down the socket;
                    // cleanup drains any prompt still waiting on the renderer.
                    cleanup(&app, &session_id, SessionStatus::Closed);
                    return;
                }
                Some(SessionCmd::Resize { cols, rows }) => {
                    queued_resize = Some((cols, rows));
                }
                Some(SessionCmd::Write(bytes)) => {
                    // Writes can't apply before ready.
                    queued_input.fetch_sub(bytes.len(), Ordering::AcqRel);
                }
            },
            changed = signal_rx.changed() => {
                let signal = *signal_rx.borrow_and_update();
                match (changed, signal) {
                    (_, SessionSignal::InputOverflow) => {
                        fail(&app, &session_id, "Too much terminal input queued; session closed".into());
                        return;
                    }
                    (Err(_), _) | (_, SessionSignal::Disconnect) => {
                        cleanup(&app, &session_id, SessionStatus::Closed);
                        return;
                    }
                    (Ok(()), SessionSignal::None) => {}
                }
            },
        }
    };

    on_ready(&app, &session_id);
    if let Some((cols, rows)) = queued_resize {
        let _ = channel.window_change(cols, rows, 0, 0).await;
    }

    // Set when the session ends because of a local failure rather than a
    // normal close; teardown below is shared either way.
    let mut error: Option<String> = None;
    loop {
        tokio::select! {
            msg = channel.wait() => {
                match msg {
                    // Server output is binary; forward raw bytes (never
                    // stringify) so multi-byte UTF-8 spanning chunk boundaries
                    // stays intact all the way to xterm.
                    Some(russh::ChannelMsg::Data { data }) => {
                        let _ = on_data.send(InvokeResponseBody::Raw(data.to_vec()));
                    }
                    Some(russh::ChannelMsg::ExtendedData { data, .. }) => {
                        let _ = on_data.send(InvokeResponseBody::Raw(data.to_vec()));
                    }
                    Some(russh::ChannelMsg::Close) | Some(russh::ChannelMsg::Eof) | None => {
                        break;
                    }
                    Some(_) => {} // exit-status etc. — close follows
                }
            }
            cmd = cmd_rx.recv() => {
                match cmd {
                    Some(SessionCmd::Write(bytes)) => {
                        let sent = channel.data(&bytes[..]).await;
                        queued_input.fetch_sub(bytes.len(), Ordering::AcqRel);
                        if sent.is_err() { break; }
                    }
                    Some(SessionCmd::Resize { cols, rows }) => {
                        let _ = channel.window_change(cols, rows, 0, 0).await;
                    }
                    None => break,
                }
            },
            changed = signal_rx.changed() => {
                let signal = *signal_rx.borrow_and_update();
                match (changed, signal) {
                    (_, SessionSignal::InputOverflow) => {
                        error = Some("Too much terminal input queued; session closed".into());
                        break;
                    }
                    (Err(_), _) | (_, SessionSignal::Disconnect) => {
                        let _ = channel.eof().await;
                        break;
                    }
                    (Ok(()), SessionSignal::None) => {}
                }
            }
        }
    }

    let _ = handle
        .disconnect(russh::Disconnect::ByApplication, "", "en")
        .await;
    match error {
        Some(message) => fail(&app, &session_id, message),
        None => cleanup(&app, &session_id, SessionStatus::Closed),
    }
}

async fn open_shell(
    handle: &russh::client::Handle<handler::ClientHandler>,
    cols: u32,
    rows: u32,
) -> Result<russh::Channel<russh::client::Msg>, russh::Error> {
    let channel = handle.channel_open_session().await?;
    channel
        .request_pty(false, "xterm-256color", cols, rows, 0, 0, &[])
        .await?;
    channel.request_shell(false).await?;
    Ok(channel)
}
