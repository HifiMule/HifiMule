; JellyfinSync NSIS installer hooks
; Registers jellyfinsync-daemon as a startup application via HKCU Run key.

!macro NSIS_HOOK_POSTINSTALL
  ; Register daemon as a startup application (runs on user login)
  WriteRegStr HKCU "Software\Microsoft\Windows\CurrentVersion\Run" "${PRODUCTNAME}" "$INSTDIR\jellyfinsync-daemon.exe"
  ; Record install location for smoke tests and tooling
  WriteRegStr HKCU "Software\JellyfinSync" "InstallDir" "$INSTDIR"
!macroend

!macro NSIS_HOOK_PREUNINSTALL
  ; Remove daemon startup registration on uninstall
  DeleteRegValue HKCU "Software\Microsoft\Windows\CurrentVersion\Run" "${PRODUCTNAME}"
  DeleteRegValue HKCU "Software\JellyfinSync" "InstallDir"
!macroend
