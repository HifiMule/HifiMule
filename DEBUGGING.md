# Debugging the Jellysync Daemon

This guide provides several methods for debugging the `jellysync-daemon`.

## 1. VS Code Integrated Debugging (Recommended)

The project includes pre-configured launch targets in `.vscode/launch.json`. To use them:
1. Open the **Run and Debug** view in VS Code (`Ctrl+Shift+D`).
2. Select **Debug executable 'jellysync-daemon'** from the dropdown.
3. Press `F5` to start debugging.
4. You can set breakpoints, inspect variables, and step through the code.

> [!NOTE]
> This uses `lldb` under the hood. Ensure you have the **C/C++** or **CodeLLDB** extension installed.

## 2. Command Line Debugging

You can run the daemon directly from your terminal to see logs and output.

```powershell
cargo run -p jellysync-daemon
```

- **Logging**: The daemon currently uses `println!` and `eprintln!` for logging. These will appear directly in your terminal.
- **Backtraces**: For detailed crash reports, run with:
  ```powershell
  $env:RUST_BACKTRACE=1; cargo run -p jellysync-daemon
  ```

## 3. Debugging via JSON-RPC

The daemon runs a JSON-RPC server on `127.0.0.1:19140`. You can interact with it using `curl` or any HTTP client (like Postman or Thunder Client).

### Example: Test Connection
```powershell
curl -X POST http://127.0.0.1:19140/ `
  -H "Content-Type: application/json" `
  -d '{
    "jsonrpc": "2.0",
    "method": "test_connection",
    "params": {
      "url": "http://your-jellyfin-url",
      "token": "your-api-token"
    },
    "id": 1
  }'
```

### Available RPC Methods:
- `test_connection`: Validate Jellyfin server URL and API key.
- `save_credentials`: Save credentials to local storage (`.jellysync.json`).
- `get_credentials`: Retrieve currently stored credentials.

## 4. Running Tests

To run the unit tests for the daemon:

```powershell
cargo test -p jellysync-daemon
```

Alternatively, use the VS Code target: **Debug unit tests in executable 'jellysync-daemon'**.

## 5. System Tray Icon

In debug mode, the daemon still shows a system tray icon.
- **Right-click** the icon to see options like **Open UI** and **Quit**.
- **Hover** over the icon to see the current status (Idle, Syncing, Scanning, etc.).

> [!TIP]
> If you need more granular logging (e.g., DEBUG/TRACE levels), consider adding the `tracing` or `log` crate in a future PR.
