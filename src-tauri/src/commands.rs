//! Tauri command surface. Thin: validate, delegate to state.

use tauri::ipc::{Channel, InvokeResponseBody};
use tauri::{AppHandle, Manager, State};

use crate::error::Error;
use crate::secrets::password_id;
use crate::state::AppState;
use crate::types::{ConnectRequest, HostEntry, TerminalSize};

/// The renderer picks the sessionId so it can route this session's status
/// and error events before the connection has even started.
#[tauri::command]
pub fn ssh_connect(
    app: AppHandle,
    state: State<'_, AppState>,
    session_id: String,
    mut req: ConnectRequest,
    size: Option<TerminalSize>,
    on_data: Channel<InvokeResponseBody>,
) -> Result<(), Error> {
    // If a password wasn't supplied but one is saved, inject it (the
    // decrypted value never leaves the backend). It is already saved, so
    // there is nothing to persist on success.
    if req.password.is_none() {
        if let Some(saved) = state
            .secrets
            .get(&password_id(&req.host, req.port, &req.username))
        {
            req.password = Some(saved);
            req.save_password = None;
        }
    }

    state
        .sessions
        .connect(app.clone(), session_id, req, size, on_data)?;
    log::debug!("live sessions: {} (connect)", state.sessions.size());
    Ok(())
}

#[tauri::command]
pub fn ssh_disconnect(state: State<'_, AppState>, session_id: String) {
    state.sessions.disconnect(&session_id);
}

#[tauri::command]
pub fn ssh_input(state: State<'_, AppState>, session_id: String, data: String) {
    state.sessions.write(&session_id, &data);
}

#[tauri::command]
pub fn ssh_resize(state: State<'_, AppState>, session_id: String, cols: u32, rows: u32) {
    state.sessions.resize(&session_id, cols, rows);
}

#[tauri::command]
pub fn hostkey_decision(state: State<'_, AppState>, session_id: String, accept: bool) {
    if let Some(tx) = state.pending.host_key.lock().unwrap().remove(&session_id) {
        let _ = tx.send(accept);
    }
}

// --- Secrets (presence/forget only; plaintext never crosses the bridge) ---

#[tauri::command]
pub fn secret_has_password(
    state: State<'_, AppState>,
    host: String,
    port: u16,
    username: String,
) -> bool {
    state.secrets.has(&password_id(&host, port, &username))
}

#[tauri::command]
pub fn secret_forget_password(
    state: State<'_, AppState>,
    host: String,
    port: u16,
    username: String,
) {
    state.secrets.delete(&password_id(&host, port, &username));
}

// --- Saved hosts ---

#[tauri::command]
pub fn hosts_list(state: State<'_, AppState>) -> Vec<HostEntry> {
    state.hosts.lock().unwrap().list()
}

#[tauri::command]
pub fn hosts_save(state: State<'_, AppState>, entry: HostEntry) -> Result<HostEntry, String> {
    state.hosts.lock().unwrap().save(entry)
}

/// Delete a saved host, and its saved password unless another saved host
/// still uses the same user@host:port (the password is keyed by those).
#[tauri::command]
pub fn hosts_delete(state: State<'_, AppState>, id: String) -> Result<(), String> {
    let mut hosts = state.hosts.lock().unwrap();
    let Some(removed) = hosts.delete(&id)? else {
        return Ok(());
    };
    if !hosts.list().iter().any(|h| h.same_target(&removed)) {
        state
            .secrets
            .delete(&password_id(&removed.host, removed.port, &removed.username));
    }
    Ok(())
}

/// Everything `run()` registers, in one place.
pub fn handlers() -> impl Fn(tauri::ipc::Invoke) -> bool {
    tauri::generate_handler![
        ssh_connect,
        ssh_disconnect,
        ssh_input,
        ssh_resize,
        hostkey_decision,
        secret_has_password,
        secret_forget_password,
        hosts_list,
        hosts_save,
        hosts_delete
    ]
}

/// Initialize managed state once the app (and its paths) are ready.
pub fn init_state(app: &AppHandle) -> Result<(), Box<dyn std::error::Error>> {
    let data_dir = app.path().app_data_dir()?;
    app.manage(AppState::new(data_dir));
    Ok(())
}
