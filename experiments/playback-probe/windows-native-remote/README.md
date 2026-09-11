# Windows native transport test client

Independent, Windows-only test helper for `test_session.py --windows-native-remote`.
Build with `cargo build --manifest-path windows-native-remote/Cargo.toml` on Windows,
or cross-compile for the tested `aarch64-pc-windows-gnullvm` target.
Run in the same interactive desktop session as the silent probe.

`status`, `pause` and `play` discover the synthetic title
`Generated silent continuity fixtures` using Windows GlobalSystemMediaTransportControlsSessionManager.
Pause and Play use that session's OS API, not the probe's controller endpoint.
Run only one generated-fixture probe at a time. Nonzero exit means missing session,
unsupported action, rejected command or Windows API failure. The harness bounds each invocation.
This proves native transport delivery; it does not simulate or validate physical media keys.
