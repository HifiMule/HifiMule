; HifiMule NSIS installer hooks
; Preserves an existing startup opt-in without enrolling fresh/disabled users.

!macro NSIS_HOOK_POSTINSTALL
  ReadRegStr $0 HKCU "Software\Microsoft\Windows\CurrentVersion\Run" "${PRODUCTNAME}"
  StrCmp $0 "" +2
  WriteRegStr HKCU "Software\Microsoft\Windows\CurrentVersion\Run" "${PRODUCTNAME}" "$INSTDIR\hifimule-daemon.exe"
  ; Record install location for smoke tests and tooling
  WriteRegStr HKCU "Software\HifiMule" "InstallDir" "$INSTDIR"
!macroend

!macro NSIS_HOOK_PREUNINSTALL
  ; Remove daemon startup registration on uninstall
  DeleteRegValue HKCU "Software\Microsoft\Windows\CurrentVersion\Run" "${PRODUCTNAME}"
  DeleteRegValue HKCU "Software\HifiMule" "InstallDir"
!macroend
