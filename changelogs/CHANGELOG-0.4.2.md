# HifiMule v0.4.2

## Bug Fixes

### MTP device connection is more reliable on Windows

Two issues that caused MTP devices to fail during initialization have been fixed.

- **Transient WPD open errors are now retried.** Windows Portable Devices (WPD) `device.Open()` can transiently return `FILE_NOT_FOUND` or `GEN_FAILURE` while the driver is still enumerating a freshly connected device. The daemon now retries up to four times with an exponential back-off (200 ms × attempt), recovering silently in most cases rather than surfacing a connection error.

- **Liveness probe no longer hangs on large MTP storage.** During device initialization a liveness probe is issued to detect stale IO from a device that disconnected and reconnected before setup completed. For MSC devices the probe is a root directory listing, but for MTP devices that same call asks WPD to recursively walk the entire phone storage, which can take many seconds. MTP devices now use `storage_id()` as a lightweight probe instead, and the resolved storage ID is reused for the rest of initialization so it is only fetched once.
