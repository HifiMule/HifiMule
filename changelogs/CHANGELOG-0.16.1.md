# HifiMule 0.16.1

Release date: 2026-09-27

## Highlights

- **Reliable Windows startup**: Fresh Windows installations register the background service to start when you sign in.
- **Safer uninstall**: The Windows uninstallers ask the background service to finish its work and wait for it to exit before removing files.
- **Installer fixes**: The NSIS same-version uninstall choice now exits cleanly, and the MSI sets the Start menu shortcut identity without the shortcut property warning.

---

## Changed

- Windows NSIS and MSI installations register the background service for the installing user's sign-in session, including fresh installations.
- Windows uninstallers request a graceful background-service shutdown before removing its executable and audio runtime files, and report a failure if shutdown does not complete.

---

## Fixed

- Choosing uninstall from the NSIS same-version reinstall prompt now ends the installer flow after uninstalling.
- The MSI Start menu shortcut no longer uses a shortcut property write that could produce Windows Installer warning 1946; its application identity is set after shortcut creation.

---

## Internal

- Added Windows daemon commands for authenticated shutdown of the installed instance and retrying Start menu shortcut identity registration.
- Extended Windows installer configuration tests and the MSI smoke test to check shortcut warnings and daemon exit after uninstall.
