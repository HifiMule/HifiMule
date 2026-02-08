import { Window } from '@tauri-apps/api/window';

// JellyfinSync UI Main Entry Point
// Coordinates splash screen and main window lifecycle.

async function init() {
    console.log("init() called, path:", window.location.pathname);

    // If we are on the splashscreen page
    if (window.location.pathname.includes('splashscreen')) {
        console.log("Detected Splashscreen window");
        try {
            const mainWin = await Window.getByLabel('main');
            const splashWin = await Window.getByLabel('splashscreen');
            console.log("Windows found:", { mainWin: !!mainWin, splashWin: !!splashWin });
            initSplashScreen(mainWin, splashWin);
        } catch (e) {
            console.error("Failed to get windows:", e);
            // Fallback: try to init anyway if UI elements are there
            initSplashScreen(null, null);
        }
        return;
    }

    // If we are on the main page (index.html)
    console.log("JellyfinSync Hub Initialized");
    document.body.classList.add('ready');
}

async function initSplashScreen(mainWin: Window | null, splashWin: Window | null) {
    console.log("initSplashScreen started");
    const statusEl = document.getElementById('status-text');
    const container = document.getElementById('container');

    if (!statusEl) {
        console.error("Status element not found!");
        return;
    }

    const timeout = 10000;
    const startTime = Date.now();
    const rpcPort = (import.meta as any).env?.VITE_RPC_PORT || '19140';
    let isPolling = false;

    const poll = async () => {
        if (isPolling) {
            console.log("Poll already in progress, skipping...");
            return;
        }

        isPolling = true;
        try {
            statusEl.textContent = "Connecting to Daemon...";
            console.log("Polling daemon...");

            const response = await fetch(`http://localhost:${rpcPort}`, {
                method: 'POST',
                headers: { 'Content-Type': 'application/json' },
                body: JSON.stringify({
                    jsonrpc: "2.0",
                    method: "get_daemon_state",
                    params: {},
                    id: 1
                })
            });

            if (response.ok) {
                console.log("Daemon responded!");
                statusEl.textContent = "Daemon Ready...";

                try {
                    // Show main window and close splash
                    if (mainWin) {
                        console.log("Showing main window");
                        await mainWin.show();
                    }
                    if (splashWin) {
                        console.log("Closing splash screen");
                        await splashWin.close();
                    }
                    return; // Successfully finished
                } catch (winError) {
                    console.error("Window API Error (Permissions?):", winError);
                    statusEl.textContent = "UI API Error - Check Console";
                    // Don't return, let it timeout or try again if appropriate
                }
            }
        } catch (e) {
            console.log("Daemon not reachable yet...");
        } finally {
            isPolling = false;
        }

        if (Date.now() - startTime > timeout) {
            console.log("Timeout reached");
            if (container) container.classList.add('error');
            statusEl.textContent = "Failed to connect to Daemon. Please ensure it is running.";
            return;
        }

        setTimeout(poll, 1000);
    };

    poll();
}

window.addEventListener('DOMContentLoaded', init);
