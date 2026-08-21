//! Tauri command surface. Thin: validate, delegate to state.

use tauri::ipc::{Channel, InvokeResponseBody};
use tauri::{AppHandle, Manager, State};

use crate::secrets::password_id;
use crate::state::{AppState, PendingSecretSave};
use crate::types::{ConnectRequest, HostEntry, TerminalSize};

#[tauri::command]
pub fn ssh_connect(
    app: AppHandle,
    state: State<'_, AppState>,
    mut req: ConnectRequest,
    size: Option<TerminalSize>,
    on_data: Channel<InvokeResponseBody>,
) -> String {
    // If a password wasn't supplied but one is saved, inject it (the
    // decrypted value never leaves the backend).
    if req.password.is_none() {
        if let Some(saved) = state
            .secrets
            .get(&password_id(&req.host, req.port, &req.username))
        {
            req.password = Some(saved);
        }
    }

    let session_id = state
        .sessions
        .connect(app.clone(), req.clone(), size, on_data);

    if req.save_password == Some(true) {
        if let Some(password) = &req.password {
            state.pending_secret_saves.lock().unwrap().insert(
                session_id.clone(),
                PendingSecretSave {
                    id: password_id(&req.host, req.port, &req.username),
                    value: password.clone(),
                },
            );
        }
    }
    log::debug!("live sessions: {} (connect)", state.sessions.size());
    session_id
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

#[tauri::command]
pub fn hosts_delete(state: State<'_, AppState>, id: String) -> Result<(), String> {
    state.hosts.lock().unwrap().delete(&id)
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
