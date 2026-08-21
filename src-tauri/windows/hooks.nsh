; NSIS installer hooks (wired via bundle.windows.nsis.installerHooks).
;
; On a real uninstall, remove everything the app ever stored: the app-data
; directories (hosts.json, known_hosts.json, WebView2 cache) and the saved
; passwords in Windows Credential Manager (keyring targets end in
; ".simplerssh": "<id>.<service>", e.g. "pw:user@host:22.simplerssh").
;
; $UpdateMode guards all of it: the installer re-runs the uninstaller with
; /UPDATE when upgrading, and an upgrade must never wipe user data. This
; mirrors the template's own checkbox branch but runs unconditionally, so
; silent uninstalls (/S) are covered too.

!macro NSIS_HOOK_POSTUNINSTALL
  ${If} $UpdateMode <> 1
    ; App data (same paths the built-in "Delete app data" checkbox removes).
    SetShellVarContext current
    RmDir /r "$APPDATA\${BUNDLEID}"
    RmDir /r "$LOCALAPPDATA\${BUNDLEID}"

    ; Install-location / installer-language registry leftovers.
    DeleteRegKey SHCTX "${MANUPRODUCTKEY}"
    DeleteRegKey /ifempty SHCTX "${MANUKEY}"
    DeleteRegValue HKCU "${MANUPRODUCTKEY}" "Installer Language"
    DeleteRegKey /ifempty HKCU "${MANUPRODUCTKEY}"
    DeleteRegKey /ifempty HKCU "${MANUKEY}"

    ; Saved secrets: delete every generic credential whose target ends in
    ; ".simplerssh". cmdkey /list prints "Target: LegacyGeneric:target=<name>".
    nsExec::ExecToLog `powershell -NoProfile -NonInteractive -ExecutionPolicy Bypass -Command "cmdkey /list | Select-String 'target=(.+\.simplerssh)\s*$$' | ForEach-Object { cmdkey /delete:($$_.Matches[0].Groups[1].Value) }"`
    Pop $0
  ${EndIf}
!macroend
