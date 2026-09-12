use std::ffi::OsString;
use std::time::Duration;
use windows_service::{
    define_windows_service,
    service::{
        ServiceAccess, ServiceErrorControl, ServiceInfo, ServiceStartType, ServiceState,
        ServiceType,
    },
    service_dispatcher,
    service_manager::{ServiceManager, ServiceManagerAccess},
};

const SERVICE_NAME: &str = "hifimule-daemon";
const SERVICE_TYPE: ServiceType = ServiceType::OWN_PROCESS;

define_windows_service!(ffi_service_main, daemon_service_main);

/// Entry point: calls StartServiceCtrlDispatcher to register with the SCM.
pub fn run() -> anyhow::Result<()> {
    service_dispatcher::start(SERVICE_NAME, ffi_service_main)
        .map_err(|e| anyhow::anyhow!("Failed to start service dispatcher: {}", e))
}

/// Register the daemon as a Windows Service using the Windows Service API.
pub fn install() -> anyhow::Result<()> {
    let exe_path = std::env::current_exe()?;
    crate::daemon_log!("--install-service: exe_path={}", exe_path.display());

    let manager =
        ServiceManager::local_computer(None::<&str>, ServiceManagerAccess::CREATE_SERVICE)
            .map_err(|e| {
                crate::daemon_log!("--install-service: failed to open SCM: {}", e);
                e
            })?;

    crate::daemon_log!("--install-service: SCM opened, creating service");

    let service_info = ServiceInfo {
        name: OsString::from(SERVICE_NAME),
        display_name: OsString::from("HifiMule Daemon"),
        service_type: SERVICE_TYPE,
        start_type: ServiceStartType::AutoStart,
        error_control: ServiceErrorControl::Normal,
        executable_path: exe_path,
        launch_arguments: vec![OsString::from("--service")],
        dependencies: vec![],
        account_name: None, // LocalSystem — needed for user keyring access
        account_password: None,
    };

    // Try to open existing service first (upgrade path), fall back to create
    let service = match manager.open_service(
        SERVICE_NAME,
        ServiceAccess::START
            | ServiceAccess::CHANGE_CONFIG
            | ServiceAccess::STOP
            | ServiceAccess::QUERY_STATUS,
    ) {
        Ok(existing) => {
            crate::daemon_log!("--install-service: service already exists, updating config");
            // Stop existing service before updating
            let _ = existing.stop();
            for _ in 0..10 {
                if let Ok(status) = existing.query_status() {
                    if status.current_state == ServiceState::Stopped {
                        break;
                    }
                }
                std::thread::sleep(Duration::from_millis(500));
            }
            existing.change_config(&service_info).map_err(|e| {
                crate::daemon_log!("--install-service: change_config failed: {}", e);
                e
            })?;
            existing
        }
        Err(_) => manager
            .create_service(
                &service_info,
                ServiceAccess::START | ServiceAccess::CHANGE_CONFIG,
            )
            .map_err(|e| {
                crate::daemon_log!("--install-service: create_service failed: {}", e);
                e
            })?,
    };

    crate::daemon_log!("--install-service: setting description");

    let _ = service.set_description("Background sync service for HifiMule media synchronization");

    crate::daemon_log!("--install-service: starting service");

    if let Err(e) = service.start::<OsString>(&[]) {
        crate::daemon_log!("--install-service: start failed (may need reboot): {}", e);
    }

    crate::daemon_log!("--install-service: completed successfully");
    Ok(())
}

/// Unregister the daemon Windows Service using the Windows Service API.
pub fn uninstall() -> anyhow::Result<()> {
    crate::daemon_log!("--uninstall-service: opening SCM");

    let manager = ServiceManager::local_computer(None::<&str>, ServiceManagerAccess::CONNECT)
        .map_err(|e| {
            crate::daemon_log!("--uninstall-service: failed to open SCM: {}", e);
            e
        })?;

    let service = manager
        .open_service(
            SERVICE_NAME,
            ServiceAccess::STOP | ServiceAccess::DELETE | ServiceAccess::QUERY_STATUS,
        )
        .map_err(|e| {
            crate::daemon_log!("--uninstall-service: open_service failed: {}", e);
            e
        })?;

    // Stop the service if running
    crate::daemon_log!("--uninstall-service: stopping service");
    let _ = service.stop();

    // Wait briefly for stop
    for _ in 0..10 {
        if let Ok(status) = service.query_status() {
            if status.current_state == ServiceState::Stopped {
                break;
            }
        }
        std::thread::sleep(Duration::from_millis(500));
    }

    // Delete the service
    crate::daemon_log!("--uninstall-service: deleting service");
    service.delete().map_err(|e| {
        crate::daemon_log!("--uninstall-service: delete failed: {}", e);
        e
    })?;

    crate::daemon_log!("--uninstall-service: completed successfully");
    Ok(())
}

fn daemon_service_main(_args: Vec<OsString>) {
    if let Err(e) = run_service() {
        crate::daemon_log!("Service error: {}", e);
    }
}

fn run_service() -> anyhow::Result<()> {
    anyhow::bail!(
        "LEGACY_DAEMON_RUNNING: Windows service mode cannot own a signed-in desktop profile; uninstall the legacy service and launch HifiMule normally"
    )
}
