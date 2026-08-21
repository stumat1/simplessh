import type { ConnectRequest, HostEntry } from '@shared/types'

export interface ConnectFormHandle {
  /** Root element to mount into a tab pane. */
  element: HTMLElement
  show: () => void
  hide: () => void
  focus: () => void
  setError: (message: string) => void
  setBusy: (busy: boolean) => void
  /** Pre-fill the fields from a previous request (secrets are not restored into inputs). */
  prefill: (req: ConnectRequest) => void
  dispose: () => void
}

/** Broadcast so every open connect form refreshes its saved-hosts list. */
const HOSTS_CHANGED = 'simplerssh:hosts-changed'
function announceHostsChanged(): void {
  window.dispatchEvent(new CustomEvent(HOSTS_CHANGED))
}

// Static markup (no interpolation of untrusted data; CSP-safe).
const TEMPLATE = `
  <form class="card connect-card" autocomplete="off">
    <h2>New SSH Connection</h2>

    <section class="saved-section hidden">
      <div class="section-label">Saved hosts</div>
      <ul class="host-list"></ul>
    </section>

    <label>Name <input class="f-name" type="text" placeholder="e.g. NAS (used if saved)" /></label>
    <label>Host <input class="f-host" type="text" placeholder="192.168.1.10" required /></label>
    <label>Port <input class="f-port" type="number" value="22" min="1" max="65535" /></label>
    <label>Username <input class="f-user" type="text" required /></label>
    <label>Password <input class="f-pass" type="password" /></label>
    <label class="checkbox"><input class="f-save-pass" type="checkbox" /> Save password</label>
    <label class="checkbox"><input class="f-save-host" type="checkbox" /> Save this host</label>

    <p class="hint f-hint"></p>
    <p class="error" role="alert"></p>
    <button type="submit">Connect</button>
  </form>
`

/**
 * Creates a self-contained connect view (its own DOM) so each tab can host
 * one. Password auth only; lists saved hosts for one-click reconnect. Calls
 * `onConnect` with a validated request.
 */
export function createConnectForm(onConnect: (req: ConnectRequest) => void): ConnectFormHandle {
  const view = document.createElement('div')
  view.className = 'overlay'
  view.innerHTML = TEMPLATE

  const q = <T extends HTMLElement>(sel: string): T => {
    const node = view.querySelector<T>(sel)
    if (!node) throw new Error(`connect-form: missing ${sel}`)
    return node
  }

  const form = q<HTMLFormElement>('form')
  const name = q<HTMLInputElement>('.f-name')
  const host = q<HTMLInputElement>('.f-host')
  const port = q<HTMLInputElement>('.f-port')
  const user = q<HTMLInputElement>('.f-user')
  const pass = q<HTMLInputElement>('.f-pass')
  const savePass = q<HTMLInputElement>('.f-save-pass')
  const saveHost = q<HTMLInputElement>('.f-save-host')
  const hintEl = q<HTMLElement>('.f-hint')
  const errorEl = q<HTMLElement>('.error')
  const submit = q<HTMLButtonElement>('button[type="submit"]')

  const savedSection = q<HTMLElement>('.saved-section')
  const hostListEl = q<HTMLUListElement>('.host-list')

  // --- Saved-password hint --------------------------------------------------
  async function refreshHint(): Promise<void> {
    const h = host.value.trim()
    const u = user.value.trim()
    const p = Number(port.value) || 22
    if (h && u && (await window.ssh.hasPassword(h, p, u))) {
      hintEl.textContent = 'A saved password will be used. Type a new one to replace it.'
      pass.placeholder = '•••••••• (saved)'
      return
    }
    pass.placeholder = ''
    hintEl.textContent = ''
  }

  for (const inputEl of [host, port, user]) {
    inputEl.addEventListener('change', () => void refreshHint())
  }

  // --- Building requests -----------------------------------------------------
  async function handleSubmit(): Promise<void> {
    errorEl.textContent = ''
    const h = host.value.trim()
    const u = user.value.trim()
    const p = Number(port.value) || 22

    if (!h || !u) {
      errorEl.textContent = 'Host and username are required.'
      return
    }
    if (!pass.value && !(await window.ssh.hasPassword(h, p, u))) {
      errorEl.textContent = 'Enter a password (none is saved for these credentials).'
      return
    }

    if (saveHost.checked) {
      const entryName = name.value.trim()
      if (!entryName) {
        errorEl.textContent = 'Enter a name for this host.'
        return
      }
      const entry: HostEntry = {
        id: '',
        name: entryName,
        host: h,
        port: p,
        username: u,
        savePassword: savePass.checked
      }
      try {
        await window.ssh.saveHost(entry)
        announceHostsChanged()
      } catch (err) {
        // Don't block connecting just because the host couldn't be persisted
        // to disk — let the user know and proceed anyway.
        errorEl.textContent = `Host not saved: ${err instanceof Error ? err.message : String(err)}`
      }
    }

    onConnect({
      host: h,
      port: p,
      username: u,
      password: pass.value || undefined,
      savePassword: savePass.checked
    })
  }

  form.addEventListener('submit', (event) => {
    event.preventDefault()
    void handleSubmit()
  })

  // --- Saved hosts -------------------------------------------------------

  /** Populate fields from a saved host (secrets are not restored into inputs). */
  function loadInto(entry: { name?: string; host: string; port: number; username: string }): void {
    name.value = entry.name ?? ''
    host.value = entry.host
    port.value = String(entry.port)
    user.value = entry.username
    pass.value = ''
  }

  /**
   * Load a host into the form, then connect immediately when a password is
   * already saved for it. Otherwise focus the password field so the user
   * just types it and presses Connect.
   */
  async function activate(entry: HostEntry): Promise<void> {
    loadInto(entry)
    if (await window.ssh.hasPassword(entry.host, entry.port, entry.username)) {
      onConnect({ host: entry.host, port: entry.port, username: entry.username })
    } else {
      pass.focus()
    }
  }

  function renderHosts(entries: HostEntry[]): void {
    hostListEl.replaceChildren()
    savedSection.classList.toggle('hidden', entries.length === 0)
    for (const entry of entries) {
      const li = document.createElement('li')
      li.className = 'list-row'

      const openBtn = document.createElement('button')
      openBtn.type = 'button'
      openBtn.className = 'list-main'
      const nameSpan = document.createElement('span')
      nameSpan.className = 'list-name'
      nameSpan.textContent = entry.name
      const metaSpan = document.createElement('span')
      metaSpan.className = 'list-meta'
      metaSpan.textContent = `${entry.username}@${entry.host}:${entry.port}`
      openBtn.append(nameSpan, metaSpan)
      openBtn.addEventListener('click', () => void activate(entry))

      const del = document.createElement('button')
      del.type = 'button'
      del.className = 'list-del'
      del.title = 'Delete host'
      del.textContent = '×'
      del.addEventListener('click', (e) => {
        e.stopPropagation()
        // The in-memory entry is removed either way; only the on-disk write
        // can fail, so log it but still refresh the (now-stale) list.
        window.ssh
          .deleteHost(entry.id)
          .catch((err: unknown) => console.error('Failed to delete host:', err))
          .then(announceHostsChanged)
      })

      li.append(openBtn, del)
      hostListEl.appendChild(li)
    }
  }

  async function refreshHosts(): Promise<void> {
    renderHosts(await window.ssh.listHosts())
  }

  const onHostsChanged = (): void => void refreshHosts()
  window.addEventListener(HOSTS_CHANGED, onHostsChanged)

  return {
    element: view,
    show: () => {
      view.style.display = 'flex'
      void refreshHosts()
      void refreshHint()
    },
    hide: () => {
      view.style.display = 'none'
    },
    focus: () => host.focus(),
    setError: (message) => {
      errorEl.textContent = message
    },
    setBusy: (busy) => {
      submit.disabled = busy
      submit.textContent = busy ? 'Connecting…' : 'Connect'
    },
    prefill: (req) => loadInto({ host: req.host, port: req.port, username: req.username }),
    dispose: () => {
      window.removeEventListener(HOSTS_CHANGED, onHostsChanged)
    }
  }
}
