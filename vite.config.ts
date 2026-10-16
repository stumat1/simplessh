import { resolve } from 'node:path'
import { defineConfig } from 'vite'

// Vite config for the renderer; Tauri runs it via beforeDevCommand /
// beforeBuildCommand (see src-tauri/tauri.conf.json).
export default defineConfig({
  root: 'src/renderer',
  resolve: {
    alias: {
      '@shared': resolve(__dirname, 'src/shared')
    }
  },
  build: {
    outDir: resolve(__dirname, 'dist-web'),
    emptyOutDir: true
  },
  // Tauri expects a fixed dev port and fails fast if it's taken.
  server: {
    port: 5173,
    strictPort: true
  },
  clearScreen: false
})
