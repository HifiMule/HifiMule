; HifiMule NSIS installer hooks
; Start the daemon in the installing user's interactive session at login.

; The daemon loads the bundled audio DLLs. Stop it before Tauri copies any
; resources so an upgrade does not first fail on a locked DLL.
!macro NSIS_HOOK_PREINSTALL
  !insertmacro CheckIfAppIsRunning "hifimule-daemon.exe" "${PRODUCTNAME}"
!macroend

!macro NSIS_HOOK_POSTINSTALL
  WriteRegStr HKCU "Software\Microsoft\Windows\CurrentVersion\Run" "${PRODUCTNAME}" "$INSTDIR\hifimule-daemon.exe"
  ; Record install location for smoke tests and tooling
  WriteRegStr HKCU "Software\HifiMule" "InstallDir" "$INSTDIR"
!macroend

!macro NSIS_HOOK_PREUNINSTALL
  ; Ask the daemon to finish playback/session cleanup and wait for its exact
  ; process to exit before the uninstaller removes its executable and DLLs.
  IfFileExists "$INSTDIR\hifimule-daemon.exe" 0 daemon_stopped
  ExecWait '"$INSTDIR\hifimule-daemon.exe" --quit' $0
  StrCmp $0 0 daemon_stopped
  Abort "HifiMule daemon did not shut down; close it before uninstalling."
  daemon_stopped:
  ; Remove daemon startup registration on uninstall
  DeleteRegValue HKCU "Software\Microsoft\Windows\CurrentVersion\Run" "${PRODUCTNAME}"
  DeleteRegValue HKCU "Software\HifiMule" "InstallDir"
!macroend
