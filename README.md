# simplessh

A minimal, secure SSH client for Windows — built with [Tauri](https://tauri.app/),
[russh](https://github.com/warp-tech/russh), and [xterm.js](https://xtermjs.org/).

Just enough to SSH into home-lab boxes: tabbed sessions, a small saved-host list,
password auth, host-key verification, and encrypted saved passwords. No port
forwarding, no key/agent/MFA auth, no themes or settings dialog — pared down on
purpose.

## Features

- **Tabbed sessions** — open and switch between a few SSH connections at once.
- **Saved hosts** — save name/host/port/username for one-click reconnect.
- **Password auth** — including servers that route it through PAM/keyboard-
  interactive (auto-answered transparently). Multi-factor/OTP auth isn't
  supported.
- **Encrypted secret storage** — saved passwords live in the Windows Credential
  Manager; plaintext is never written to disk.
- **Host-key verification** — unknown and changed host keys prompt for an
  explicit trust decision (TOFU), with SHA-256 fingerprints.
- **Auto-reconnect** — a dropped session shows a one-click reconnect overlay.
- **Terminal niceties** — right-click copy/paste menu, clickable links.

## Requirements

- Windows 10/11 (x64)
- For building from source: [Node.js](https://nodejs.org/) 20.19+ (or 22.12+)
  with npm, and a [Rust toolchain](https://rustup.rs/) (stable, MSVC)

## Getting started (development)

```bash
npm install
npm run dev
```

A throwaway SSH server for manual testing is included:

```bash
node scripts/test-sshd.cjs 2222   # user "test", password "secret123"
```

## Building

```bash
npm run build
```

This typechecks, bundles the UI, compiles the Rust backend in release mode, and
produces `src-tauri/target/release/simplerssh.exe` (standalone) plus an NSIS
installer under `src-tauri/target/release/bundle/nsis/`.

App data (saved hosts, known hosts) lives in `%APPDATA%\com.simplerssh.app`;
saved passwords live in the Windows Credential Manager. Uninstalling via the
NSIS installer removes both.

## Scripts

| Script               | Description                                  |
| --------------------- | --------------------------------------------- |
| `npm run dev`          | Run the app in development with hot reload    |
| `npm run build`        | Build the release app + NSIS installer        |
| `npm run typecheck`    | Typecheck the renderer and config TypeScript  |
| `npm run lint`         | Run ESLint                                    |
| `npm run format`       | Format sources with Prettier                  |
| `cargo test` (in `src-tauri/`) | Run Rust backend unit tests           |

## Project structure

```
src/
├── renderer/    UI — tabs, terminal, connect form, host-key dialog (vanilla TS)
└── shared/      Types shared between the renderer and the API bridge
src-tauri/
└── src/         Rust backend — SSH sessions (russh), saved hosts, secrets, known-hosts
```

The renderer talks to the backend through a single typed surface (`window.ssh`,
see `src/shared/api.ts`), implemented over Tauri commands and events in
`src/renderer/ssh-api.ts`. Terminal output streams over a per-session raw-byte
IPC channel.

## Security

- The UI runs in a WebView with a strict Content-Security-Policy; all
  privileged work (network, file system, secrets) happens in the Rust backend.
- Saved passwords live in the Windows Credential Manager and are only
  persisted on a successful connection (and only when you opt in). Plaintext
  never crosses into the UI.
- Host keys are verified on every connection; changes are surfaced loudly.

Use this software only to access systems you own or are explicitly authorized
to access.

## License

MIT
