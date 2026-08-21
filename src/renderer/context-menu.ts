// Lightweight custom context menu. One menu at a time; dismissed on click-away,
// Esc, scroll, resize, or choosing an item.

export interface ContextMenuItem {
  label: string
  enabled?: boolean
  action: () => void
}

export type ContextMenuEntry = ContextMenuItem | 'separator'

let openMenu: HTMLElement | null = null
let teardown: (() => void) | null = null

export function closeContextMenu(): void {
  openMenu?.remove()
  openMenu = null
  teardown?.()
  teardown = null
}

/** Show a context menu at viewport coordinates, clamped to stay on screen. */
export function showContextMenu(x: number, y: number, entries: ContextMenuEntry[]): void {
  closeContextMenu()
  const opener = document.activeElement as HTMLElement | null

  const menu = document.createElement('div')
  menu.className = 'context-menu'
  menu.setAttribute('role', 'menu')

  for (const entry of entries) {
    if (entry === 'separator') {
      const sep = document.createElement('div')
      sep.className = 'context-menu-sep'
      menu.appendChild(sep)
      continue
    }
    const item = document.createElement('button')
    item.type = 'button'
    item.className = 'context-menu-item'
    item.setAttribute('role', 'menuitem')
    item.textContent = entry.label
    item.disabled = entry.enabled === false
    item.addEventListener('click', () => {
      closeContextMenu()
      entry.action()
    })
    menu.appendChild(item)
  }

  document.body.appendChild(menu)

  // Clamp so the menu never overflows the window.
  const rect = menu.getBoundingClientRect()
  menu.style.left = `${Math.min(x, window.innerWidth - rect.width - 4)}px`
  menu.style.top = `${Math.min(y, window.innerHeight - rect.height - 4)}px`

  const onPointerDown = (e: PointerEvent): void => {
    if (!menu.contains(e.target as Node)) closeContextMenu()
  }
  const onKeyDown = (e: KeyboardEvent): void => {
    if (e.key === 'Escape') {
      e.preventDefault()
      closeContextMenu()
      opener?.focus()
    }
  }
  const onDismiss = (): void => closeContextMenu()
  // Capture phase so a click on the terminal (which stops propagation) still closes it.
  window.addEventListener('pointerdown', onPointerDown, true)
  window.addEventListener('keydown', onKeyDown, true)
  window.addEventListener('blur', onDismiss)
  window.addEventListener('resize', onDismiss)

  openMenu = menu
  teardown = () => {
    window.removeEventListener('pointerdown', onPointerDown, true)
    window.removeEventListener('keydown', onKeyDown, true)
    window.removeEventListener('blur', onDismiss)
    window.removeEventListener('resize', onDismiss)
  }
}
