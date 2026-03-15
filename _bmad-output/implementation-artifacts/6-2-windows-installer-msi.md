# Story 6.2: Windows Installer (MSI)

Status: ready-for-dev

<!-- Note: Validation is optional. Run validate-create-story for quality check before dev-story. -->

## Story

As a **Ritualist (Arthur)**,
I want a standard Windows MSI installer that registers the daemon as a startup application,
so that I can install JellyfinSync like any other desktop application on my Windows PC, with the daemon auto-launching on login with full tray icon support.

## Acceptance Criteria

1. **MSI installs to Program Files**: Given a successful `cargo tauri build` on Windows, when I run the generated MSI, then JellyfinSync is installed to `C:\Program Files\JellyfinSync\` with Start Menu shortcuts.
2. **Daemon sidecar co-located**: The daemon sidecar (`jellyfinsync-daemon.exe`) is placed alongside the main executable in the installation directory.
3. **Clean uninstallation**: Uninstallation via "Add/Remove Programs" cleanly removes all installed files from Program Files, Start Menu entries, and the Registry `Run` key.
4. **Start Menu shortcuts functional**: Start Menu shortcut launches JellyfinSync correctly (UI starts, daemon sidecar spawns).
5. **App metadata correct**: The installer shows correct product name ("JellyfinSync"), manufacturer, version, and icon in Add/Remove Programs.
6. **Daemon registered as startup application**: Given the MSI installation completes, when the installer registers the startup entry, then `jellyfinsync-daemon` is registered via a Registry `Run` key (`HKCU\Software\Microsoft\Windows\CurrentVersion\Run`). The daemon launches automatically when the user logs in, running in the user session with full tray icon and notification support.
7. **UI detects running daemon**: The UI detects the running daemon via a health-check RPC call instead of spawning a sidecar. If the daemon is not running, the UI attempts to launch it directly.
8. **Startup entry removed on uninstall**: Uninstallation removes the Registry `Run` key entry.

## Tasks / Subtasks

- [ ] **T1: Validate Current WiX MSI Output** (AC: #1, #2, #5)
  - [ ] T1.1: Run `cargo tauri build` and locate the generated MSI in `target/release/bundle/msi/`
  - [ ] T1.2: Install the MSI and verify installation directory is `C:\Program Files\JellyfinSync\`
  - [ ] T1.3: Verify `jellyfinsync-daemon.exe` is present alongside `JellyfinSync.exe` in the install directory
  - [ ] T1.4: Verify application icon, name, and version display correctly in Add/Remove Programs
  - [ ] T1.5: Document any issues found during validation

- [ ] **T2: Configure Windows-Specific Bundle Settings** (AC: #1, #3, #5)
  - [ ] T2.1: Review and configure `bundle.windows` section in `tauri.conf.json` for MSI-specific settings (if refinements needed based on T1 findings)
  - [ ] T2.2: Ensure WiX `UpgradeCode` GUID is stable for upgrade support (currently `44585dad-44ac-5c08-ad8d-e5a7a7dfcb10`)
  - [ ] T2.3: Verify `InstallScope` is set appropriately (`perMachine` for Program Files installation)
  - [ ] T2.4: Confirm the MSI includes proper `MajorUpgrade` element to handle upgrades without requiring manual uninstall

- [ ] **T3: Validate Start Menu & Shortcuts** (AC: #4)
  - [ ] T3.1: Verify Start Menu shortcut is created under `Start Menu\Programs\JellyfinSync\`
  - [ ] T3.2: Launch JellyfinSync from Start Menu shortcut and confirm the UI window appears
  - [ ] T3.3: Verify the daemon sidecar starts (check `localhost:19140` responds to health check)
  - [ ] T3.4: Verify `System.AppUserModel.ID` is set to `com.alexi.jellyfinsync` on shortcuts for proper taskbar grouping

- [ ] **T4: Validate Clean Uninstallation** (AC: #3, #8)
  - [ ] T4.1: Uninstall via Add/Remove Programs (`msiexec /x`)
  - [ ] T4.2: Verify all files removed from `C:\Program Files\JellyfinSync\` (including daemon sidecar)
  - [ ] T4.3: Verify Start Menu shortcuts are removed
  - [ ] T4.4: Verify registry entries under `HKCU\Software\alexi\JellyfinSync` are cleaned up
  - [ ] T4.5: Verify the `HKCU\Software\Microsoft\Windows\CurrentVersion\Run\JellyfinSync` key is removed
  - [ ] T4.6: Verify `%APPDATA%\JellyfinSync\` app data is NOT deleted by default (user data preservation)

- [ ] **T5: Register Daemon as Startup Application** (AC: #6, #8)
  - [ ] T5.1: Create a WiX fragment that writes a Registry `Run` key (`HKCU\Software\Microsoft\Windows\CurrentVersion\Run`) pointing to `jellyfinsync-daemon.exe` in the install directory
  - [ ] T5.2: Use WiX `<RegistryValue>` element with `Type="string"` and `Value="[INSTALLDIR]jellyfinsync-daemon.exe"` — no custom actions needed
  - [ ] T5.3: Ensure the registry entry is automatically cleaned up on uninstall (WiX handles this natively for `<RegistryValue>` in a `<Component>`)
  - [ ] T5.4: Replace the existing WiX service fragment (`wix/service-fragment.wxs`) with the new startup registry fragment in `tauri.conf.json` `fragmentPaths` and `componentGroupRefs`
  - [ ] T5.5: Verify after MSI install that `HKCU\...\Run\JellyfinSync` points to the correct daemon exe path
  - [ ] T5.6: Verify after reboot/re-login that the daemon auto-starts with tray icon visible

- [ ] **T6: Update UI Daemon Detection Labels** (AC: #7)
  - [ ] T6.1: Keep the existing 3-tier detection (health check → `sc start` → sidecar) — a power user may have manually registered the service via `--install-service`
  - [ ] T6.2: Update `get_sidecar_status` to return `"startup"` instead of `"service"` when connected to an already-running daemon via health check (default case is now the startup app, not the service)
  - [ ] T6.3: Verify UI correctly displays daemon connection mode

## Dev Notes

### Architecture & Technical Requirements

- **Tauri v2 WiX Bundler:** Tauri v2 uses WiX Toolset v3 to generate MSI installers. Custom WiX fragments can be added via `bundle.windows.wix.fragmentPaths`. The existing service fragment will be replaced with a simpler registry fragment.
- **Startup App vs Service:** The default installation now uses a Registry `Run` key (`HKCU\Software\Microsoft\Windows\CurrentVersion\Run`) instead of a Windows Service. This runs the daemon in the user's interactive session (Session 1+), preserving full tray icon and notification support. The Windows Service infrastructure remains in the codebase for power users who want to manually register via `jellyfinsync-daemon --install-service`.
- **Registry Run key advantages:** No elevation needed for HKCU writes, daemon runs as the logged-in user (no Session 0 isolation), tray icon works natively, simpler WiX fragment (declarative `<RegistryValue>` vs custom actions), automatic cleanup on uninstall.
- **WiX `<RegistryValue>` vs Custom Actions:** WiX natively supports declarative registry writes via `<RegistryValue>` inside a `<Component>`. When the component is uninstalled, WiX automatically removes the registry entry. No custom actions, no `sc.exe`, no deferred execution context issues.
- **Current MSI output:** Story 6.1 produces a working MSI. The existing service fragment (`wix/service-fragment.wxs`) with its Type 18 custom actions will be replaced by a far simpler registry-only fragment.

### Current WiX Configuration (from 6.1 output)

| Setting | Current Value | Notes |
|---------|--------------|-------|
| UpgradeCode | `44585dad-44ac-5c08-ad8d-e5a7a7dfcb10` | Must remain stable for MSI upgrade support |
| InstallScope | `perMachine` | Installs to Program Files (requires elevation) |
| Install Dir | `ProgramFiles64Folder\JellyfinSync` | Standard x64 location |
| Manufacturer | `alexi` | Displays in Add/Remove Programs |
| Shortcuts | Start Menu + Desktop | With `System.AppUserModel.ID` |

### WiX Startup Fragment Design

```xml
<!-- Replace wix/service-fragment.wxs with this -->
<Wix xmlns="http://schemas.microsoft.com/wix/2006/wi">
  <Fragment>
    <ComponentGroup Id="StartupRegistryGroup">
      <Component Id="StartupRegistryComponent"
                 Directory="INSTALLDIR"
                 Guid="PUT-NEW-GUID-HERE">
        <RegistryValue
          Root="HKCU"
          Key="Software\Microsoft\Windows\CurrentVersion\Run"
          Name="JellyfinSync"
          Type="string"
          Value="[INSTALLDIR]jellyfinsync-daemon.exe"
          KeyPath="yes" />
      </Component>
    </ComponentGroup>
  </Fragment>
</Wix>
```

Key points:
- `Root="HKCU"` — per-user, no elevation needed for the registry write itself (though MSI install to Program Files already requires elevation)
- `KeyPath="yes"` on the RegistryValue — WiX tracks this component by the registry key, ensuring clean removal on uninstall
- The `componentGroupRefs` in `tauri.conf.json` should reference `StartupRegistryGroup` (replacing the old service group reference)

### Previous Story Intelligence (6.1)

- `cargo tauri build` produces MSI and NSIS installers
- `prepare-sidecar.mjs` handles daemon binary preparation
- Sidecar launched in `lib.rs` `setup()` hook via `app.shell().sidecar("jellyfinsync-daemon")`
- 122+ tests pass
- `productName` = "JellyfinSync", `identifier` = "com.alexi.jellyfinsync"

### Git Intelligence

Recent commits show 6.1 and 6.2 (service version) completed, then a correct-course:
- `6dac84b` Correct course
- `c68ec88` Review for 6.2
- `cad223c` Dev 6.2
- `94e1e13` Story 6.2

Key learnings from previous 6.2 attempt:
- WiX custom actions (Type 34, Type 18) are fragile — deferred execution context issues, path resolution problems, `sc.exe` quoting bugs. The startup registry approach avoids ALL of these by using declarative `<RegistryValue>` instead.
- `componentGroupRefs` in `tauri.conf.json` is required for WiX to include the fragment (linker drops unreferenced fragments silently).
- The 3-tier daemon detection in `lib.rs` (health check → sc start → sidecar) works well and should be preserved.

### What NOT to Do

- Do NOT remove the Windows Service code (`service.rs`, `--service`/`--install-service`/`--uninstall-service` flags) — it stays for power users
- Do NOT change the NSIS installer — this story is MSI-only
- Do NOT modify the daemon's RPC protocol
- Do NOT remove the sidecar launch fallback from `lib.rs` — still needed for non-MSI installs and dev mode
- Do NOT use WiX custom actions for the registry write — use declarative `<RegistryValue>` instead
- Do NOT write to `HKLM\...\Run` — use `HKCU` so the daemon runs in the user's session, not as a system-wide process

### Project Structure Notes

- Workspace: `jellyfinsync-daemon` (standalone Rust binary) + `jellyfinsync-ui/src-tauri` (Tauri Rust backend)
- Frontend: `jellyfinsync-ui/src/` (Vanilla TypeScript + Shoelace)
- Tauri config: `jellyfinsync-ui/src-tauri/tauri.conf.json`
- Icons: `jellyfinsync-ui/src-tauri/icons/`
- Build output: `target/release/bundle/msi/` (MSI), `target/release/bundle/nsis/` (NSIS)
- Sidecar staging: `jellyfinsync-ui/src-tauri/sidecars/`
- WiX fragments: `jellyfinsync-ui/src-tauri/wix/` (replace `service-fragment.wxs` with startup registry fragment)
- App data (runtime): `%APPDATA%/JellyfinSync/` (daemon.log, ui.log, jellyfinsync.db)
- Rust edition 2021, MSRV 1.93.0

### Key Files to Modify

| File | Change |
|------|--------|
| `jellyfinsync-ui/src-tauri/wix/service-fragment.wxs` | Replace with startup registry fragment (rename to `startup-fragment.wxs`) |
| `jellyfinsync-ui/src-tauri/tauri.conf.json` | Update `fragmentPaths` and `componentGroupRefs` to reference new fragment |
| `jellyfinsync-ui/src-tauri/src/lib.rs` | Update `SidecarStatus` default label from `"service"` to `"startup"` for health-check-detected daemon |

### References

- [Source: planning-artifacts/epics.md#story-62-windows-installer-msi] — Epic Requirements (Post-MVP: Daemon as Windows Startup Application)
- [Source: planning-artifacts/architecture.md#structure-patterns] — Packaging & Distribution patterns
- [Source: 6-1-tauri-bundler-configuration-sidecar-packaging.md] — Previous story: sidecar configuration, build script
- [Source: jellyfinsync-ui/src-tauri/tauri.conf.json] — Current Tauri configuration
- [Source: jellyfinsync-ui/src-tauri/wix/service-fragment.wxs] — Current WiX service fragment (to be replaced)
- [Source: jellyfinsync-ui/src-tauri/src/lib.rs] — Current 3-tier daemon detection logic

## Dev Agent Record

### Agent Model Used

{{agent_model_name_version}}

### Debug Log References

### Completion Notes List

### File List
