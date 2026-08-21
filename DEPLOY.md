# Deploying simplerssh

## Requirements

- Windows 10/11 (x64)
- [Node.js](https://nodejs.org/) 20.19+ (or 22.12+) with npm
- A [Rust toolchain](https://rustup.rs/) (stable, MSVC)

## Running in development

```bash
npm install
npm run dev
```

`npm run dev` launches the app via the Tauri CLI with hot reload for the UI
(Vite) and automatic rebuild for the Rust backend. A throwaway local SSH
server is included for testing without touching a real host:

```bash
node scripts/test-sshd.cjs 2222   # user "test", password "secret123"
```

## Building a release

```bash
npm run build
```

This typechecks, bundles the UI, compiles the Rust backend in release mode,
and produces:

```
src-tauri/target/release/
├── simplerssh.exe                                # standalone, self-contained app
└── bundle/nsis/simplerssh_<version>_x64-setup.exe # per-user NSIS installer
```

Both are unsigned unless you've set up code signing yourself — Windows Smart
App Control (or SmartScreen) may flag an unsigned installer on first run.

## Installing on Windows

Two options, both from the `src-tauri/target/release/` output above:

- **Installer** — run `simplerssh_<version>_x64-setup.exe`. Installs
  per-user (no admin required), adds a Start-menu shortcut, and registers an
  uninstaller.
- **Portable** — copy `simplerssh.exe` anywhere and run it directly. UI
  assets are embedded in the binary and WebView2 ships with Windows, so no
  installation step is needed.

### Data locations

- App data (saved hosts, known hosts): `%APPDATA%\com.simplerssh.app`
- Saved passwords: Windows Credential Manager, under targets ending in
  `.simplerssh`

### Uninstalling

- **Installer**: use "Apps & Features" (or the Start-menu uninstaller
  shortcut). This removes the app files, the app-data folder, and every saved
  password. Upgrading in place (installing a newer version over an older one)
  does **not** trigger this cleanup — only a real uninstall does.
- **Portable**: delete `simplerssh.exe`, then manually remove
  `%APPDATA%\com.simplerssh.app` and any `*.simplerssh` entries in Credential
  Manager if you want the saved hosts/passwords gone too.
