// Shared domain types used across the Rust backend and the renderer.

export interface HostEntry {
  id: string
  name: string
  host: string
  port: number
  username: string
  /** Persist the password on a successful connect. */
  savePassword: boolean
}

/** A transient request to open a connection (not necessarily a saved host). */
export interface ConnectRequest {
  host: string
  port: number
  username: string
  password?: string
  /** Persist the password (encrypted) after a successful connection. */
  savePassword?: boolean
}

/** Sent to the renderer when a host key needs the user's trust decision. */
export interface HostKeyPrompt {
  sessionId: string
  host: string
  port: number
  /** SHA256 fingerprint of the presented key. */
  fingerprint: string
  /** 'unknown' = never seen; 'changed' = differs from a previously trusted key. */
  status: 'unknown' | 'changed'
  /** For 'changed': the fingerprint we had on file. */
  knownFingerprint?: string
}

/** Initial PTY dimensions sent with a connect request. */
export interface TerminalSize {
  cols: number
  rows: number
}

export type SessionStatus = 'connecting' | 'ready' | 'closed' | 'error'
