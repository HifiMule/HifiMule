; HifiMule NSIS installer hooks
; Registers hifimule-daemon as a startup application via HKCU Run key.

!macro NSIS_HOOK_POSTINSTALL
  ; Register daemon as a startup application (runs on user login)
  WriteRegStr HKCU "Software\Microsoft\Windows\CurrentVersion\Run" "${PRODUCTNAME}" "$INSTDIR\hifimule-daemon.exe"
  ; Record install location for smoke tests and tooling
  WriteRegStr HKCU "Software\HifiMule" "InstallDir" "$INSTDIR"
!macroend

!macro NSIS_HOOK_PREUNINSTALL
  ; Remove daemon startup registration on uninstall
  DeleteRegValue HKCU "Software\Microsoft\Windows\CurrentVersion\Run" "${PRODUCTNAME}"
  DeleteRegValue HKCU "Software\HifiMule" "InstallDir"
!macroend
