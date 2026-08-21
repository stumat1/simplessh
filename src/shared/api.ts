// The contract for the privileged API exposed to the renderer as
// `window.ssh`, implemented over Tauri IPC in src/renderer/ssh-api.ts.
import type { ConnectRequest, HostEntry, HostKeyPrompt, SessionStatus, TerminalSize } from './types'

export type DataListener = (sessionId: string, data: Uint8Array) => void
export type StatusListener = (sessionId: string, status: SessionStatus) => void
export type ErrorListener = (sessionId: string, message: string) => void
export type HostKeyListener = (prompt: HostKeyPrompt) => void

export interface SshApi {
  getVersion(): Promise<string>

  /** Open an SSH session + PTY shell. Resolves with the new sessionId. */
  connect(req: ConnectRequest, size?: TerminalSize): Promise<string>

  /** Request a graceful disconnect of a session. */
  disconnect(sessionId: string): Promise<void>

  /** Send keystrokes to a session's shell. */
  sendInput(sessionId: string, data: string): void

  /** Notify the remote PTY of a new terminal size. */
  resize(sessionId: string, cols: number, rows: number): void

  /** Stream of raw output bytes from a session. Returns an unsubscribe fn. */
  onData(cb: DataListener): () => void

  /** Session lifecycle status updates. */
  onStatus(cb: StatusListener): () => void

  /** Connection/auth error messages for a session. */
  onError(cb: ErrorListener): () => void

  /** A host key needs the user's trust decision. */
  onHostKeyPrompt(cb: HostKeyListener): () => void

  /** Answer a host-key prompt for a session. */
  hostKeyDecision(sessionId: string, accept: boolean): Promise<void>

  /** Whether a password is saved for these credentials (no plaintext exposed). */
  hasPassword(host: string, port: number, username: string): Promise<boolean>

  /** Forget a saved password. */
  forgetPassword(host: string, port: number, username: string): Promise<void>

  /** List saved home-lab hosts. */
  listHosts(): Promise<HostEntry[]>

  /** Create or update a saved host; resolves with the stored entry. */
  saveHost(entry: HostEntry): Promise<HostEntry>

  /** Delete a saved host by id. */
  deleteHost(id: string): Promise<void>

  /** Read the system clipboard as text. */
  clipboardRead(): Promise<string>

  /** Write text to the system clipboard. */
  clipboardWrite(text: string): Promise<void>
}
