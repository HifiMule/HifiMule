use serde::Serialize;

#[derive(Debug, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum SupportState {
    Available,
    Missing,
    Unknown,
    NotApplicable,
}

#[derive(Serialize)]
pub struct TraySupport {
    state: SupportState,
    fedora: bool,
}

#[tauri::command]
pub async fn get_tray_support() -> TraySupport {
    #[cfg(target_os = "linux")]
    {
        tauri::async_runtime::spawn_blocking(linux_probe)
            .await
            .unwrap_or(TraySupport { state: SupportState::Unknown, fedora: false })
    }
    #[cfg(not(target_os = "linux"))]
    {
        TraySupport { state: SupportState::NotApplicable, fedora: false }
    }
}

fn is_gnome(desktop: &str) -> bool {
    desktop.split([':', ';']).any(|token| token.trim().eq_ignore_ascii_case("gnome"))
}

fn classify_probe(host: Option<bool>, remote_error: Option<&str>) -> SupportState {
    match host {
        Some(true) => SupportState::Available,
        Some(false) => SupportState::Missing,
        None if matches!(remote_error, Some("org.freedesktop.DBus.Error.ServiceUnknown" | "org.freedesktop.DBus.Error.NameHasNoOwner")) => SupportState::Missing,
        None => SupportState::Unknown,
    }
}

#[cfg(target_os = "linux")]
fn linux_probe() -> TraySupport {
    use gio::prelude::*;
    use glib::{translate::{from_glib_full, ToGlibPtr}, variant::ToVariant};
    let fedora = std::fs::read_to_string("/etc/os-release").map(|text| text.lines().any(|line| {
        line.strip_prefix("ID=").is_some_and(|id| id.trim_matches(['\"', '\'']).eq_ignore_ascii_case("fedora"))
    })).unwrap_or(false);
    let desktop = std::env::var("XDG_CURRENT_DESKTOP").ok().filter(|value| !value.trim().is_empty())
        .or_else(|| std::env::var("XDG_SESSION_DESKTOP").ok()).unwrap_or_default();
    if !is_gnome(&desktop) { return TraySupport { state: SupportState::NotApplicable, fedora }; }
    // The deadline also bounds connecting to an unavailable session bus.
    let cancel = gio::Cancellable::new();
    let timer_cancel = cancel.clone();
    let (done, wait) = std::sync::mpsc::channel();
    let timer = std::thread::spawn(move || {
        if wait.recv_timeout(std::time::Duration::from_secs(2)).is_err() { timer_cancel.cancel(); }
    });
    let result = gio::bus_get_sync(gio::BusType::Session, Some(&cancel)).and_then(|bus| bus.call_sync(
        Some("org.kde.StatusNotifierWatcher"), "/StatusNotifierWatcher", "org.freedesktop.DBus.Properties", "Get",
        Some(&("org.kde.StatusNotifierWatcher", "IsStatusNotifierHostRegistered").to_variant()),
        None, gio::DBusCallFlags::NO_AUTO_START, 1500, Some(&cancel),
    ));
    let _ = done.send(());
    let _ = timer.join();
    let state = match result {
        Ok(reply) => classify_probe(reply.get::<(glib::Variant,)>().and_then(|(value,)| value.get::<bool>()), None),
        Err(error) => {
            // Compare the structured remote name, never an error message substring.
            let name: Option<glib::GString> = unsafe { from_glib_full(gio::ffi::g_dbus_error_get_remote_error(error.to_glib_none().0)) };
            classify_probe(None, name.as_deref())
        }
    };
    TraySupport { state, fedora }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn desktop_tokens() {
        for desktop in ["GNOME", "gnome", "ubuntu:GNOME", "GNOME:Unity", " GNOME ;other"] { assert!(is_gnome(desktop)); }
        for desktop in ["", "KDE", "X-GNOME", "gnome-classic", "Windows", "macOS"] { assert!(!is_gnome(desktop)); }
    }
    #[test]
    fn injected_probe_results() {
        assert_eq!(classify_probe(Some(true), None), SupportState::Available);
        assert_eq!(classify_probe(Some(false), None), SupportState::Missing);
        for error in ["org.freedesktop.DBus.Error.ServiceUnknown", "org.freedesktop.DBus.Error.NameHasNoOwner"] {
            assert_eq!(classify_probe(None, Some(error)), SupportState::Missing);
        }
        for error in [None, Some("org.freedesktop.DBus.Error.NoReply"), Some("org.freedesktop.DBus.Error.Disconnected"), Some("org.freedesktop.DBus.Error.AccessDenied")] {
            assert_eq!(classify_probe(None, error), SupportState::Unknown);
        }
    }
    #[cfg(target_os = "linux")]
    #[test]
    fn malformed_reply_is_unknown() {
        use glib::variant::ToVariant;
        for value in ["bad".to_variant(), 42u32.to_variant(), ("bad",).to_variant()] {
            assert_eq!(classify_probe(value.get::<(glib::Variant,)>().and_then(|(v,)| v.get::<bool>()), None), SupportState::Unknown);
        }
        for host in [true, false] {
            let reply = (host.to_variant(),).to_variant();
            assert_eq!(classify_probe(reply.get::<(glib::Variant,)>().and_then(|(v,)| v.get::<bool>()), None), if host { SupportState::Available } else { SupportState::Missing });
        }
    }
}
