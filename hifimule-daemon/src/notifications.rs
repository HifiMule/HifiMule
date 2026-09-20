use notify_rust::Notification;

#[cfg(windows)]
pub fn new_notification() -> Notification {
    let mut notification = Notification::new();
    notification.app_id("hifimule.github.io");
    notification
}

#[cfg(not(windows))]
pub fn new_notification() -> Notification {
    Notification::new()
}

#[cfg(test)]
mod tests {
    #[test]
    #[cfg(windows)]
    fn notifications_use_the_hifimule_windows_application_identity() {
        let notification = super::new_notification();

        assert!(format!("{notification:?}").contains("app_id: Some(\"hifimule.github.io\")"));
    }
}
