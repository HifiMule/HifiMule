## Before opening an issue: collect the logs

HifiMule’s logs help us diagnose startup, connection, playback, and synchronization problems:

- **`daemon.log`** records background activity.
- **`ui.log`** records application startup and communication with the daemon.

Reproduce the problem, note the approximate time, and copy the logs soon afterward. Logs are automatically truncated when they exceed approximately 1 MB, so older information may disappear.

### Where to find the logs

| Platform | `daemon.log` | `ui.log` |
|---|---|---|
| **Windows** | `%APPDATA%\HifiMule\daemon.log` | `%APPDATA%\HifiMule\ui.log` |
| **macOS** | `~/Library/Application Support/HifiMule/daemon.log` | `~/Library/Application Support/HifiMule/ui.log` |
| **Linux** | `~/.local/share/HifiMule/daemon.log` | Currently not written to a file. |

**Windows:** Press **Win + R**, enter `%APPDATA%\HifiMule`, and press **Enter**.

**macOS:** In Finder, press **Command + Shift + G**, enter `~/Library/Application Support/HifiMule`, and press **Return**.

**Linux:** Open `~/.local/share/HifiMule` in your file manager. If you have configured `XDG_DATA_HOME`, look in `$XDG_DATA_HOME/HifiMule` instead. For UI diagnostics, launch HifiMule from a terminal and copy the output produced while reproducing the problem.

If you explicitly configured `HIFIMULE_APP_DATA_DIR`, look for `daemon.log` in that directory. On Windows and macOS, `ui.log` still uses the default location listed above.

If a log is missing, mention that in the issue and include whichever logs are available.

### Remove personal information before sharing

**Issues and attachments may be public. Review the logs before posting them.**

Make a copy of each log and edit the copies in a plain-text editor. Look for information you do not want to share, including:

- Your name, username, or email address.
- Local file paths containing personal folder names.
- Server addresses, hostnames, and IP addresses.
- Device names or serial numbers.
- Private media titles or filenames.
- Passwords, API keys, access tokens, cookies, or authorization headers, if present.

Replace sensitive values with clear placeholders such as `[USER]`, `[SERVER]`, or `[TOKEN]`. Use the same placeholder for repeated occurrences of the same value.

For example:

```text
C:\Users\Alice\Music\Private album\track.flac
→ C:\Users\[USER]\Music\[ALBUM]\track.flac

https://music.example.com/stream?token=secret-value
→ https://[SERVER]/stream?token=[TOKEN]
```

Keep timestamps, error messages, error codes, and surrounding log lines whenever possible: they help us understand what happened. Search for each sensitive value again after editing to catch repeated occurrences.

Share only the reviewed logs or relevant excerpts. Avoid uploading the entire HifiMule data folder, database, configuration, or `runtime` directory, which may contain private information or authentication secrets.

### What to include in your issue

Include your HifiMule version, operating system, steps to reproduce, expected behavior, actual behavior, and the approximate time of the problem. Attach the sanitized logs, or paste relevant excerpts inside a fenced code block:

````markdown
```text
Paste sanitized log lines here.
```
````
