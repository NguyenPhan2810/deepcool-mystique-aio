; Installer hooks for the MYSTIQUE Control NSIS package.
;
; Included by tauri-bundler before the installer sections, so these macros run from
; `installer.nsi` itself. Keep them declarative: no custom pages, no template copy.
;
; The install is per-user (`installMode: currentUser`): the copy needs no elevation.
; The one exception is the vendor service, whose start type only an administrator may
; change. That is the only UAC prompt this package raises.

!include "LogicLib.nsh"

; The UI finds `mystique-daemon.exe` beside its own executable. The resource is
; bundled under `daemon\` (see `tauri.conf.json`) so that a debug build does not try to
; overwrite the running daemon in `target\debug`; this hook moves it into place after
; the file copy, before the finish page can launch the app.
;
; A failed rename is a real installation failure: the UI would otherwise be installed
; without its daemon. The usual cause is a daemon left running by an earlier version,
; so the message says how to clear it.
!macro NSIS_HOOK_POSTINSTALL
  ; An older daemon may still be running after the UI was closed (it deliberately
  ; outlives the window) and would hold the file we are about to replace. The Restart
  ; Manager prompt names the process and offers to close it; a fresh install finds
  ; nothing and shows no prompt.
  !insertmacro CheckIfAppIsRunning "$INSTDIR\mystique-daemon.exe" "${PRODUCTNAME}"

  ${IfNot} ${FileExists} "$INSTDIR\daemon\mystique-daemon.exe"
    MessageBox MB_ICONSTOP "The daemon was not included in this package. The installation is incomplete." /SD IDOK
    Abort
  ${EndIf}

  Delete "$INSTDIR\mystique-daemon.exe"
  ClearErrors
  Rename "$INSTDIR\daemon\mystique-daemon.exe" "$INSTDIR\mystique-daemon.exe"
  ${If} ${Errors}
    MessageBox MB_ICONSTOP "mystique-daemon.exe could not be installed next to mystique-ui.exe. Stop any running mystique-daemon process and run the installer again." /SD IDOK
    Abort
  ${EndIf}
  Delete "$INSTDIR\daemon\.gitignore"
  RMDir "$INSTDIR\daemon"

  ; Stop Windows from starting DeepCool's `MystiqueService` at boot. Our daemon owns
  ; the panel; the vendor service is set to manual (`demand`) so it is still available
  ; if the user deliberately starts it.
  ;
  ; `sc query` needs no elevation, so ask first: a machine without the vendor service
  ; should not see a UAC prompt for a command that would do nothing. A missing service
  ; returns 1060 and is reported in the log, not treated as an installation failure.
  nsExec::ExecToStack '"$SYSDIR\sc.exe" query MystiqueService'
  Pop $R9
  Pop $R8
  ${If} $R9 == 0
    ; `sc config` needs elevation even though the installer does not, so it is launched
    ; with the `runas` verb: UAC asks once, and only for this command. The result is
    ; deliberately not checked. A declined prompt makes ShellExecuteEx fail; that is not
    ; a reason to fail an otherwise complete installation.
    DetailPrint "Setting MystiqueService to manual start (a UAC prompt may appear)..."
    ClearErrors
    ExecShell "runas" "$SYSDIR\sc.exe" "config MystiqueService start= demand" SW_HIDE
  ${Else}
    DetailPrint "MystiqueService is not installed; leaving the service configuration alone."
  ${EndIf}
!macroend

; The uninstaller only knows the bundled `daemon\` path, so remove the sibling the
; install hook created. `MystiqueService` is left exactly as it is: the vendor stack is
; not ours to remove.
;
; The "Delete app data" checkbox is checked only when the user asks (or
; `/DELETEAPPDATA` is passed). Honour that choice for our own per-user state too. The
; daemon's log is kept even here; only the settings file the user created is removed,
; and the directory only if nothing else remains in it.
!macro NSIS_HOOK_POSTUNINSTALL
  ; A daemon left running by the UI would otherwise keep the file and the directory
  ; alive. `/REBOOTOK` removes them on the next restart if they are locked now.
  Delete /REBOOTOK "$INSTDIR\mystique-daemon.exe"
  RMDir /REBOOTOK "$INSTDIR\daemon"
  ; The generated uninstaller removes $INSTDIR before this hook runs, so the sibling
  ; deleted above is what kept the directory alive. Try once more now that it is gone.
  RMDir /REBOOTOK "$INSTDIR"

  ${If} $DeleteAppDataCheckboxState = 1
    Delete "$LOCALAPPDATA\mystique\settings.json"
    RMDir "$LOCALAPPDATA\mystique"
  ${EndIf}
!macroend
