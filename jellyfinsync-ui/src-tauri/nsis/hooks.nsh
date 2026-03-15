; JellyfinSync NSIS installer hooks
; Registers jellyfinsync-daemon as a startup application via HKCU Run key.

!macro NSIS_HOOK_POSTINSTALL
  ; Register daemon as a startup application (runs on user login)
  WriteRegStr HKCU "Software\Microsoft\Windows\CurrentVersion\Run" "${PRODUCTNAME}" "$INSTDIR\jellyfinsync-daemon.exe"
!macroend
