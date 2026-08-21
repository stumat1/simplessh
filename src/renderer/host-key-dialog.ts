import type { HostKeyPrompt } from '@shared/types'

export interface HostKeyDialogOptions {
  /**
   * Move keyboard focus into the dialog. Pass false when the prompt belongs to
   * a background tab so the user's typing elsewhere isn't hijacked.
   */
  takeFocus?: boolean
}

export interface HostKeyDialog {
  /** Show the prompt; resolves true if the user trusts the key. */
  prompt: (info: HostKeyPrompt, opts?: HostKeyDialogOptions) => Promise<boolean>
}

function el<T extends HTMLElement>(id: string): T {
  const node = document.getElementById(id)
  if (!node) throw new Error(`Missing element #${id}`)
  return node as T
}

const FOCUSABLE = 'a[href], button:not(:disabled), input:not(:disabled), [tabindex]:not([tabindex="-1"])'

/** Trap Tab/Shift+Tab inside `container` until the returned dispose() is called. */
function trapFocus(container: HTMLElement): () => void {
  const onKeyDown = (e: KeyboardEvent): void => {
    if (e.key !== 'Tab') return
    const focusable = Array.from(container.querySelectorAll<HTMLElement>(FOCUSABLE))
    if (focusable.length === 0) return
    const first = focusable[0]
    const last = focusable[focusable.length - 1]
    const active = document.activeElement as HTMLElement | null
    if (e.shiftKey && (active === first || !container.contains(active))) {
      e.preventDefault()
      last.focus()
    } else if (!e.shiftKey && active === last) {
      e.preventDefault()
      first.focus()
    }
  }
  container.addEventListener('keydown', onKeyDown)
  return () => container.removeEventListener('keydown', onKeyDown)
}

const UNKNOWN_MSG =
  'You are connecting to this server for the first time. Confirm the fingerprint matches the one shown by the server before continuing.'
const CHANGED_MSG =
  'WARNING: the host key does not match the one previously trusted. This may indicate a man-in-the-middle attack. Only continue if you know the key changed legitimately (e.g. the server was rebuilt).'

export function setupHostKeyDialog(): HostKeyDialog {
  const view = el<HTMLElement>('hostkey-view')
  const title = el<HTMLElement>('hostkey-title')
  const hostEl = el<HTMLElement>('hostkey-host')
  const fpEl = el<HTMLElement>('hostkey-fp')
  const knownLine = el<HTMLElement>('hostkey-known-line')
  const knownEl = el<HTMLElement>('hostkey-known')
  const warning = el<HTMLElement>('hostkey-warning')
  const card = view.querySelector<HTMLElement>('.card')!
  const acceptBtn = el<HTMLButtonElement>('hostkey-accept')
  const cancelBtn = el<HTMLButtonElement>('hostkey-cancel')

  let resolver: ((value: boolean) => void) | null = null
  let releaseTrap: (() => void) | null = null

  function close(value: boolean): void {
    view.classList.add('hidden')
    releaseTrap?.()
    releaseTrap = null
    const r = resolver
    resolver = null
    r?.(value)
  }

  acceptBtn.addEventListener('click', () => close(true))
  cancelBtn.addEventListener('click', () => close(false))
  // Escape denies the key — the safe default for a trust decision.
  view.addEventListener('keydown', (e) => {
    if (e.key === 'Escape' && !view.classList.contains('hidden')) {
      e.preventDefault()
      close(false)
    }
  })

  return {
    prompt: (info, opts) =>
      new Promise<boolean>((resolve) => {
        resolver = resolve
        const changed = info.status === 'changed'

        title.textContent = changed ? '⚠ Host key changed' : 'Unknown host key'
        title.classList.toggle('danger', changed)
        // textContent (not innerHTML) — never interpolate server data as markup.
        hostEl.textContent = `${info.host}:${info.port}`
        fpEl.textContent = `SHA256:${info.fingerprint}`
        warning.textContent = changed ? CHANGED_MSG : UNKNOWN_MSG

        if (changed && info.knownFingerprint) {
          knownEl.textContent = `SHA256:${info.knownFingerprint}`
          knownLine.classList.remove('hidden')
        } else {
          knownLine.classList.add('hidden')
        }

        view.classList.remove('hidden')
        releaseTrap = trapFocus(card)
        // Default focus lands on Cancel (the safe choice for a trust decision).
        if (opts?.takeFocus !== false) cancelBtn.focus()
      })
  }
}
