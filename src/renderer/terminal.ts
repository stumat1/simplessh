import { Terminal, type ITheme } from '@xterm/xterm'
import { FitAddon } from '@xterm/addon-fit'
import { WebLinksAddon } from '@xterm/addon-web-links'
import { openUrl } from '@tauri-apps/plugin-opener'
import '@xterm/xterm/css/xterm.css'
import { showContextMenu } from './context-menu'

// Fixed dark theme aligned with the app shell (see styles.css).
const THEME: ITheme = {
  background: '#1e1e1e',
  foreground: '#d4d4d4',
  cursor: '#d4d4d4',
  cursorAccent: '#1e1e1e',
  selectionBackground: '#264f78',
  black: '#000000',
  red: '#cd3131',
  green: '#0dbc79',
  yellow: '#e5e510',
  blue: '#2472c8',
  magenta: '#bc3fbc',
  cyan: '#11a8cd',
  white: '#e5e5e5',
  brightBlack: '#666666',
  brightRed: '#f14c4c',
  brightGreen: '#23d18b',
  brightYellow: '#f5f543',
  brightBlue: '#3b8eea',
  brightMagenta: '#d670d6',
  brightCyan: '#29b8db',
  brightWhite: '#ffffff'
}

export interface TerminalInit {
  /**
   * App-level shortcut handler (new tab, close tab, …). Return true if the
   * key was consumed, in which case it is kept out of the PTY.
   */
  onAppShortcut: (e: KeyboardEvent) => boolean
}

export interface TerminalHandle {
  term: Terminal
  /** Refit the terminal grid to the current container size. */
  fit: () => void
  /** Tear down the terminal and its observers/DOM. */
  dispose: () => void
}

/**
 * Creates an xterm terminal mounted in `container`, wired with the fit and
 * web-links addons. Handles copy/paste (Ctrl+Shift+C/V) and delegates other
 * app shortcuts. Keeps the grid fitted to the container via a ResizeObserver.
 */
export function createTerminal(container: HTMLElement, init: TerminalInit): TerminalHandle {
  const term = new Terminal({
    cursorBlink: true,
    cursorStyle: 'block',
    fontFamily: "'Cascadia Mono', Consolas, monospace",
    fontSize: 14,
    lineHeight: 1.1,
    scrollback: 5000,
    theme: THEME
  })

  const fitAddon = new FitAddon()
  term.loadAddon(fitAddon)

  term.open(container)

  // Open links in the OS browser via the backend — never in the app window.
  term.loadAddon(
    new WebLinksAddon((event, uri) => {
      event.preventDefault()
      void openUrl(uri)
    })
  )

  // --- Right-click menu (the WebView default is suppressed app-wide in main.ts) ---
  const pasteFromClipboard = (): void => {
    void window.ssh.clipboardRead().then((text) => {
      if (text) term.paste(text)
      term.focus()
    })
  }
  const onContextMenu = (e: MouseEvent): void => {
    e.preventDefault()
    const selection = term.getSelection()
    showContextMenu(e.clientX, e.clientY, [
      {
        label: 'Copy',
        enabled: !!selection,
        action: () => void window.ssh.clipboardWrite(selection)
      },
      { label: 'Paste', action: pasteFromClipboard },
      'separator',
      { label: 'Select all', action: () => term.selectAll() },
      { label: 'Clear', action: () => term.clear() }
    ])
  }
  container.addEventListener('contextmenu', onContextMenu)

  // --- Keyboard: copy/paste and app shortcuts ---
  term.attachCustomKeyEventHandler((e): boolean => {
    if (e.type !== 'keydown') return true
    const ctrlShift = e.ctrlKey && e.shiftKey

    if (ctrlShift && e.code === 'KeyC') {
      const selection = term.getSelection()
      if (selection) void window.ssh.clipboardWrite(selection)
      e.preventDefault()
      return false
    }
    if (ctrlShift && e.code === 'KeyV') {
      void window.ssh.clipboardRead().then((text) => {
        if (text) term.paste(text)
      })
      e.preventDefault()
      return false
    }

    // App-level shortcuts: let the shared handler act, then keep the key out of
    // the PTY and stop it from also bubbling to the window-level listener.
    if (init.onAppShortcut(e)) {
      e.preventDefault()
      e.stopPropagation()
      return false
    }
    return true
  })

  const fit = (): void => {
    // Skip when the container has no layout box (hidden/background tab); fitting
    // a zero-size element would resize the terminal — and the remote PTY — wrongly.
    if (!container.clientWidth || !container.clientHeight) return
    try {
      fitAddon.fit()
    } catch {
      /* not measurable yet — ignore */
    }
  }

  const resizeObserver = new ResizeObserver(() => fit())
  resizeObserver.observe(container)

  fit()

  const dispose = (): void => {
    resizeObserver.disconnect()
    container.removeEventListener('contextmenu', onContextMenu)
    term.dispose()
  }

  return { term, fit, dispose }
}
