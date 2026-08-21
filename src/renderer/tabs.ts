import { createTerminal, type TerminalHandle, type TerminalInit } from './terminal'
import { createConnectForm, type ConnectFormHandle } from './connect-form'
import { getCurrentWindow } from '@tauri-apps/api/window'
import { ask } from '@tauri-apps/plugin-dialog'
import type { ConnectRequest, SessionStatus } from '@shared/types'

type TabState = 'form' | 'connecting' | 'connected' | 'disconnected'

let tabSeq = 0

/**
 * One tab = one pane containing a connect form and (once connected) an xterm
 * terminal, plus its button in the tab bar. Owns a single SSH session at a time.
 */
class Tab {
  readonly key = `tab-${++tabSeq}`
  sessionId: string | null = null
  private state: TabState = 'form'
  private title = 'New tab'
  // Last request actually sent, kept for one-click reconnect. May hold an
  // unsaved typed password in renderer memory — accepted trade-off for a
  // personal-use app.
  private lastReq: ConnectRequest | null = null
  private wasConnected = false
  private tooltip = ''
  // Cancel pressed before ssh.connect() resolved the sessionId — disconnect
  // as soon as it does.
  private cancelRequested = false

  readonly pane: HTMLElement
  readonly button: HTMLElement
  private readonly titleEl: HTMLElement
  private readonly dot: HTMLElement
  private readonly termContainer: HTMLElement
  private readonly form: ConnectFormHandle
  private readonly reconnectOverlay: HTMLElement
  private readonly reconnectMsg: HTMLElement
  private readonly reconnectBtn: HTMLButtonElement
  private readonly connectingOverlay: HTMLElement
  private readonly connectingMsg: HTMLElement
  private readonly cancelBtn: HTMLButtonElement
  private terminal: TerminalHandle | null = null

  constructor(private readonly mgr: TabManager) {
    // Pane: terminal container with the connect form overlaid on top.
    this.pane = document.createElement('div')
    this.pane.className = 'tab-pane'
    this.pane.id = `${this.key}-panel`
    this.termContainer = document.createElement('div')
    this.termContainer.className = 'tab-terminal'
    this.form = createConnectForm((req) => void this.connect(req))

    // Reconnect overlay, shown instead of the form when a live session drops.
    this.reconnectOverlay = document.createElement('div')
    this.reconnectOverlay.className = 'overlay'
    this.reconnectOverlay.style.display = 'none'
    const card = document.createElement('div')
    card.className = 'card'
    const heading = document.createElement('h2')
    heading.textContent = 'Connection closed'
    this.reconnectMsg = document.createElement('p')
    this.reconnectMsg.className = 'hint'
    const actions = document.createElement('div')
    actions.className = 'card-actions'
    this.reconnectBtn = document.createElement('button')
    this.reconnectBtn.type = 'button'
    this.reconnectBtn.textContent = 'Reconnect'
    this.reconnectBtn.addEventListener('click', () => {
      if (this.lastReq) void this.connect(this.lastReq)
    })
    const editBtn = document.createElement('button')
    editBtn.type = 'button'
    editBtn.className = 'btn-secondary'
    editBtn.textContent = 'Edit connection'
    editBtn.addEventListener('click', () => {
      this.wasConnected = false
      if (this.lastReq) this.form.prefill(this.lastReq)
      this.applyState('disconnected')
      this.form.focus()
    })
    const quitBtn = document.createElement('button')
    quitBtn.type = 'button'
    quitBtn.className = 'btn-secondary'
    quitBtn.textContent = 'Quit'
    quitBtn.addEventListener('click', () => void this.quitApp())
    actions.append(this.reconnectBtn, editBtn, quitBtn)
    card.append(heading, this.reconnectMsg, actions)
    this.reconnectOverlay.appendChild(card)

    // Connecting overlay: shows the target and a Cancel button while the
    // handshake/auth is in flight (the backend aborts the attempt on cancel).
    this.connectingOverlay = document.createElement('div')
    this.connectingOverlay.className = 'overlay'
    this.connectingOverlay.style.display = 'none'
    const connCard = document.createElement('div')
    connCard.className = 'card'
    const connHeading = document.createElement('h2')
    connHeading.textContent = 'Connecting…'
    this.connectingMsg = document.createElement('p')
    this.connectingMsg.className = 'hint'
    const connActions = document.createElement('div')
    connActions.className = 'card-actions'
    this.cancelBtn = document.createElement('button')
    this.cancelBtn.type = 'button'
    this.cancelBtn.className = 'btn-secondary'
    this.cancelBtn.textContent = 'Cancel'
    this.cancelBtn.addEventListener('click', () => this.cancelConnect())
    connActions.appendChild(this.cancelBtn)
    connCard.append(connHeading, this.connectingMsg, connActions)
    this.connectingOverlay.appendChild(connCard)

    this.pane.append(
      this.termContainer,
      this.form.element,
      this.reconnectOverlay,
      this.connectingOverlay
    )

    // Tab bar button: status dot + title + close.
    this.button = document.createElement('div')
    this.button.className = 'tab'
    this.dot = document.createElement('span')
    this.dot.className = 'tab-dot'
    this.titleEl = document.createElement('span')
    this.titleEl.className = 'tab-title'
    const closeBtn = document.createElement('button')
    closeBtn.className = 'tab-close'
    closeBtn.textContent = '×'
    closeBtn.title = 'Close tab'
    closeBtn.addEventListener('click', (e) => {
      e.stopPropagation()
      this.mgr.closeTab(this)
    })
    this.button.append(this.dot, this.titleEl, closeBtn)
    this.button.addEventListener('click', () => this.mgr.activate(this))

    this.setTitle('New tab')
    this.applyState('form')
  }

  private setTitle(title: string): void {
    this.title = title
    this.titleEl.textContent = title
    this.button.title = this.tooltip || title
  }

  private ensureTerminal(): TerminalHandle {
    if (this.terminal) return this.terminal
    const handle = createTerminal(this.termContainer, this.mgr.terminalInit())
    handle.term.onData((data) => {
      if (this.sessionId) window.ssh.sendInput(this.sessionId, data)
    })
    handle.term.onResize(({ cols, rows }) => {
      if (this.sessionId) window.ssh.resize(this.sessionId, cols, rows)
    })
    this.terminal = handle
    return handle
  }

  private async connect(req: ConnectRequest): Promise<void> {
    this.lastReq = req
    this.cancelRequested = false
    this.cancelBtn.disabled = false
    this.form.setError('')
    this.form.setBusy(true)
    this.tooltip = `${req.username}@${req.host}:${req.port}`
    this.button.title = this.tooltip
    this.connectingMsg.textContent = this.tooltip
    this.setTitle(`${req.username}@${req.host}`)
    const handle = this.ensureTerminal()
    handle.term.clear()
    this.applyState('connecting')
    // Reflect connecting state on the tab dot locally — the backend
    // 'connecting' status event fires before this tab is registered by sessionId.
    this.dot.dataset.status = 'connecting'
    try {
      const sessionId = await window.ssh.connect(req, {
        cols: handle.term.cols,
        rows: handle.term.rows
      })
      this.sessionId = sessionId
      this.mgr.registerSession(sessionId, this)
      if (this.cancelRequested) void window.ssh.disconnect(sessionId)
    } catch (err) {
      this.form.setBusy(false)
      this.wasConnected = false
      this.form.prefill(req)
      this.form.setError(err instanceof Error ? err.message : String(err))
      this.applyState('form')
    }
  }

  private async quitApp(): Promise<void> {
    if (this.mgr.hasLiveSessionsExcept(this)) {
      const ok = await ask('Other tabs still have open connections. Quit anyway?', {
        title: 'simplerssh',
        kind: 'warning'
      })
      if (!ok) return
    }
    await getCurrentWindow().close()
  }

  private cancelConnect(): void {
    this.cancelRequested = true
    this.cancelBtn.disabled = true
    // Make sure the form has something to show when the 'closed' status lands.
    if (this.lastReq) this.form.prefill(this.lastReq)
    if (this.sessionId) void window.ssh.disconnect(this.sessionId)
  }

  write(data: Uint8Array): void {
    this.terminal?.term.write(data)
  }

  setError(message: string): void {
    this.form.setError(message)
    this.reconnectMsg.textContent = message
  }

  setStatus(status: SessionStatus): void {
    this.dot.dataset.status = status
    switch (status) {
      case 'connecting':
        break
      case 'ready':
        this.form.setBusy(false)
        this.wasConnected = true
        this.applyState('connected')
        if (this.mgr.isActive(this)) {
          this.terminal?.fit()
          this.terminal?.term.focus()
        }
        break
      case 'closed':
      case 'error':
        if (this.sessionId) {
          this.mgr.unregisterSession(this.sessionId)
          this.sessionId = null
        }
        this.form.setBusy(false)
        this.terminal?.term.writeln('\r\n\x1b[90m[session closed]\x1b[0m')
        if (status === 'closed') this.reconnectMsg.textContent = 'The session ended.'
        this.applyState('disconnected')
        break
    }
    if (this.mgr.isActive(this)) this.mgr.refreshStatusBar()
  }

  /** Toggle form / reconnect overlay / terminal within the pane based on state. */
  private applyState(state: TabState): void {
    this.state = state
    // After a live session drops, offer one-click reconnect instead of the form.
    const useOverlay = state === 'disconnected' && this.wasConnected && this.lastReq !== null
    this.reconnectOverlay.style.display = useOverlay ? 'flex' : 'none'
    this.connectingOverlay.style.display = state === 'connecting' ? 'flex' : 'none'
    const showForm = (state === 'form' || state === 'disconnected') && !useOverlay
    if (showForm) this.form.show()
    else this.form.hide()
  }

  show(): void {
    this.pane.style.display = 'block'
    if (this.state === 'connected') {
      this.terminal?.fit()
      this.terminal?.term.focus()
    } else if (this.reconnectOverlay.style.display !== 'none') {
      this.reconnectBtn.focus()
    } else {
      this.form.focus()
    }
  }

  hide(): void {
    this.pane.style.display = 'none'
  }

  statusText(): string {
    switch (this.state) {
      case 'connected':
        return `Connected — ${this.title}`
      case 'connecting':
        return `Connecting — ${this.title}…`
      case 'disconnected':
        return `Disconnected — ${this.title}`
      default:
        return 'Not connected'
    }
  }

  /** Disconnect (if live) and tear down all DOM/resources. */
  dispose(): void {
    if (this.sessionId) {
      void window.ssh.disconnect(this.sessionId)
      this.mgr.unregisterSession(this.sessionId)
      this.sessionId = null
    }
    this.terminal?.dispose()
    this.terminal = null
    this.form.dispose()
    this.pane.remove()
    this.button.remove()
  }
}

export interface TabManagerOptions {
  /** Shared app-shortcut handler delegated to each terminal. */
  onAppShortcut: (e: KeyboardEvent) => boolean
}

/**
 * Manages the set of tabs: the tab bar, the active pane, and routing of SSH
 * stream events to the owning tab by sessionId.
 */
export class TabManager {
  private readonly tabs: Tab[] = []
  private active: Tab | null = null
  private readonly bySessionId = new Map<string, Tab>()

  constructor(
    private readonly tabBar: HTMLElement,
    private readonly content: HTMLElement,
    private readonly newTabButton: HTMLElement,
    private readonly statusEl: HTMLElement,
    private readonly options: TabManagerOptions
  ) {
    this.newTabButton.addEventListener('click', () => this.newTab())
  }

  terminalInit(): TerminalInit {
    return { onAppShortcut: this.options.onAppShortcut }
  }

  newTab(): Tab {
    const tab = new Tab(this)
    this.tabs.push(tab)
    this.content.appendChild(tab.pane)
    this.tabBar.insertBefore(tab.button, this.newTabButton)
    this.activate(tab)
    return tab
  }

  activate(tab: Tab): void {
    if (this.active === tab) {
      tab.show()
      return
    }
    if (this.active) {
      this.active.hide()
      this.active.button.classList.remove('active')
    }
    this.active = tab
    tab.button.classList.add('active')
    tab.show()
    this.refreshStatusBar()
  }

  /** Activate the tab `dir` steps from the active one (Ctrl+Tab), wrapping. */
  activateAdjacent(dir: -1 | 1): void {
    if (!this.active || this.tabs.length < 2) return
    const i = this.tabs.indexOf(this.active)
    const next = this.tabs[(i + dir + this.tabs.length) % this.tabs.length]
    this.activate(next)
  }

  isActive(tab: Tab): boolean {
    return this.active === tab
  }

  /** True when any tab other than `tab` still owns a live SSH session. */
  hasLiveSessionsExcept(tab: Tab): boolean {
    for (const owner of this.bySessionId.values()) {
      if (owner !== tab) return true
    }
    return false
  }

  closeTab(tab: Tab): void {
    const index = this.tabs.indexOf(tab)
    if (index < 0) return
    tab.dispose()
    this.tabs.splice(index, 1)

    if (this.active === tab) {
      this.active = null
      const next = this.tabs[index] ?? this.tabs[index - 1] ?? null
      if (next) this.activate(next)
      else this.newTab() // always keep at least one tab open
    }
    this.refreshStatusBar()
  }

  /** Close the active tab (Ctrl+W). */
  closeActive(): void {
    if (this.active) this.closeTab(this.active)
  }

  registerSession(sessionId: string, tab: Tab): void {
    this.bySessionId.set(sessionId, tab)
  }

  unregisterSession(sessionId: string): void {
    this.bySessionId.delete(sessionId)
  }

  tabForSession(sessionId: string): Tab | undefined {
    return this.bySessionId.get(sessionId)
  }

  /** Whether the session belongs to the currently active tab. */
  isSessionActive(sessionId: string): boolean {
    const tab = this.bySessionId.get(sessionId)
    return tab !== undefined && this.active === tab
  }

  refreshStatusBar(): void {
    this.statusEl.textContent = this.active ? this.active.statusText() : 'No tabs'
  }
}
