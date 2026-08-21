import './styles.css'
import { tauriSshApi } from './ssh-api'
import { TabManager } from './tabs'
import { setupHostKeyDialog } from './host-key-dialog'

function el<T extends HTMLElement>(id: string): T {
  const node = document.getElementById(id)
  if (!node) throw new Error(`Missing element #${id}`)
  return node as T
}

window.ssh = tauriSshApi

function bootstrap(): void {
  // Suppress the WebView2 default context menu everywhere; components that want
  // a menu (e.g. the terminal) attach their own contextmenu handlers.
  window.addEventListener('contextmenu', (e) => e.preventDefault())

  // App-level shortcuts shared by the window listener and each terminal so they
  // work whether the focus is on the terminal or the connect form.
  function appShortcut(e: KeyboardEvent): boolean {
    if (!e.ctrlKey || e.altKey || e.metaKey) return false
    switch (e.code) {
      case 'KeyT':
        if (e.shiftKey) return false
        tabs.newTab()
        return true
      case 'KeyW':
        if (e.shiftKey) return false
        tabs.closeActive()
        return true
      case 'Tab':
        // Ctrl+Tab / Ctrl+Shift+Tab cycle tabs.
        tabs.activateAdjacent(e.shiftKey ? -1 : 1)
        return true
      default:
        return false
    }
  }

  const tabs = new TabManager(el('tabbar'), el('content'), el('new-tab'), el('status'), {
    onAppShortcut: appShortcut
  })

  // Route SSH stream events to the owning tab by sessionId.
  window.ssh.onData((sid, data) => tabs.tabForSession(sid)?.write(data))
  window.ssh.onStatus((sid, status) => tabs.tabForSession(sid)?.setStatus(status))
  window.ssh.onError((sid, message) => tabs.tabForSession(sid)?.setError(message))

  // Host-key trust prompt. Serialize so concurrent handshakes (multiple tabs)
  // queue their dialogs instead of overlapping.
  const hostKeyDialog = setupHostKeyDialog()
  let modalChain: Promise<void> = Promise.resolve()
  const enqueueModal = (fn: () => Promise<void>): void => {
    modalChain = modalChain.then(fn).catch(() => {})
  }

  // Never force-switch to the owning tab: yanking the user mid-typing is worse
  // than answering the dialog out of context. Keyboard focus is only taken
  // when the prompt is for the tab the user is already on.
  window.ssh.onHostKeyPrompt((prompt) => {
    enqueueModal(async () => {
      const accepted = await hostKeyDialog.prompt(prompt, {
        takeFocus: tabs.isSessionActive(prompt.sessionId)
      })
      await window.ssh.hostKeyDecision(prompt.sessionId, accepted)
    })
  })

  // Window-level shortcuts (used when the terminal isn't focused, e.g. the form).
  window.addEventListener('keydown', (e) => {
    if (appShortcut(e)) e.preventDefault()
  })

  void window.ssh.getVersion().then((v) => {
    el('version').textContent = `v${v}`
  })

  tabs.newTab()
}

bootstrap()
