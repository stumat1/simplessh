// Tauri implementation of the SshApi contract, assigned to `window.ssh` by
// main.ts.
//
// Terminal output is the hot path: it streams over a per-session Tauri IPC
// Channel as raw bytes (ArrayBuffer), never JSON-serialized, so multi-byte
// UTF-8 spanning chunk boundaries stays intact and full-screen TUI redraws
// stay fast. Low-frequency traffic (status, errors, auth prompts) uses plain
// Tauri events.
import { Channel, invoke } from '@tauri-apps/api/core'
import { getVersion } from '@tauri-apps/api/app'
import { listen } from '@tauri-apps/api/event'
import { readText, writeText } from '@tauri-apps/plugin-clipboard-manager'
import type { DataListener, ErrorListener, HostKeyListener, SshApi, StatusListener } from '@shared/api'
import type { ConnectRequest, HostEntry, HostKeyPrompt, SessionStatus, TerminalSize } from '@shared/types'

const dataListeners = new Set<DataListener>()
const statusListeners = new Set<StatusListener>()
const errorListeners = new Set<ErrorListener>()
const hostKeyListeners = new Set<HostKeyListener>()

/** Add to a listener set; returns the matching unsubscribe function. */
function subscribe<T>(set: Set<T>, cb: T): () => void {
  set.add(cb)
  return () => {
    set.delete(cb)
  }
}

interface StatusEvent {
  sessionId: string
  status: SessionStatus
}
interface ErrorEvent {
  sessionId: string
  message: string
}

// One global listener per event, fanned out to subscribers — registered once
// at module load. (listen() is async; events can't fire before the WebView is
// running this code anyway.)
void listen<StatusEvent>('ssh:status', (e) => {
  for (const cb of statusListeners) cb(e.payload.sessionId, e.payload.status)
})
void listen<ErrorEvent>('ssh:error', (e) => {
  for (const cb of errorListeners) cb(e.payload.sessionId, e.payload.message)
})
void listen<HostKeyPrompt>('ssh:hostkey-prompt', (e) => {
  for (const cb of hostKeyListeners) cb(e.payload)
})

export const tauriSshApi: SshApi = {
  getVersion: () => getVersion(),

  connect: async (req: ConnectRequest, size?: TerminalSize): Promise<string> => {
    const onData = new Channel<ArrayBuffer>()
    // The channel needs the sessionId, which only exists once connect returns;
    // buffer is unnecessary because the backend emits no data before the shell
    // opens, which is well after this resolves.
    let sessionId = ''
    onData.onmessage = (buf) => {
      const bytes = new Uint8Array(buf)
      for (const cb of dataListeners) cb(sessionId, bytes)
    }
    sessionId = await invoke<string>('ssh_connect', { req, size, onData })
    return sessionId
  },

  disconnect: (sessionId) => invoke('ssh_disconnect', { sessionId }),

  sendInput: (sessionId, data) => {
    void invoke('ssh_input', { sessionId, data })
  },

  resize: (sessionId, cols, rows) => {
    void invoke('ssh_resize', { sessionId, cols, rows })
  },

  onData: (cb) => subscribe(dataListeners, cb),
  onStatus: (cb) => subscribe(statusListeners, cb),
  onError: (cb) => subscribe(errorListeners, cb),
  onHostKeyPrompt: (cb) => subscribe(hostKeyListeners, cb),

  hostKeyDecision: (sessionId, accept) => invoke('hostkey_decision', { sessionId, accept }),

  hasPassword: (host, port, username) =>
    invoke<boolean>('secret_has_password', { host, port, username }),
  forgetPassword: (host, port, username) =>
    invoke('secret_forget_password', { host, port, username }),

  listHosts: () => invoke<HostEntry[]>('hosts_list'),
  saveHost: (entry) => invoke<HostEntry>('hosts_save', { entry }),
  deleteHost: (id) => invoke('hosts_delete', { id }),

  clipboardRead: () => readText(),
  clipboardWrite: (text) => writeText(text)
}
