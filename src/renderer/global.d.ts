import type { SshApi } from '@shared/api'

// Makes `window.ssh` strongly typed in the renderer.
declare global {
  interface Window {
    ssh: SshApi
  }
}

export {}
