import { Window } from '@tauri-apps/api/window';

// HifiMule UI Main Entry Point
// Coordinates splash screen and main window lifecycle.

let activeBasketSidebar: any = null;

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
    console.log("HifiMule Hub Initialized");
    document.body.classList.add('ready');

    const { rpcCall } = await import('./rpc');

    try {
        const state = await rpcCall('get_daemon_state');
        renderMainLayout();

        if (state.serverConnected) {
            console.log("Server connected, loading library view");
            const { initLibraryView } = await import('./library');
            initLibraryView();
        } else {
            console.log("Server not connected, showing login view");
            const { initLoginView } = await import('./login');
            // const { initLibraryView } = await import('./library'); // Removed double import

            initLoginView(() => {
                console.log("Login success callback triggered");
                renderMainLayout();
                import('./library').then(({ initLibraryView }) => initLibraryView());
            });
        }
    } catch (e) {
        console.error("Failed to check daemon state", e);
        renderMainLayout();
        // Fallback to login
        const { initLoginView } = await import('./login');
        initLoginView(() => {
            renderMainLayout();
            import('./library').then(({ initLibraryView }) => initLibraryView());
        });
    }
}

function renderMainLayout() {
    const root = document.querySelector('.app-container');
    if (!root) return;

    root.innerHTML = `
    <sl-split-panel position="70" class="split-panel">
      <div slot="start" class="library-view">
        <header>
          <h1>Library</h1>
          <p>Select media to sync to your device.</p>
        </header>

        <div id="library-content" class="content">
          <!-- Media grid will be rendered here by library.ts -->
        </div>
      </div>

      <div slot="end" class="basket-view" id="basket-sidebar-container">
        <!-- BasketSidebar component will render here -->
      </div>
    </sl-split-panel>
    `;

    // Initialize Basket Sidebar
    import('./components/BasketSidebar').then(({ BasketSidebar }) => {
        if (activeBasketSidebar) {
            activeBasketSidebar.destroy();
        }
        const container = document.getElementById('basket-sidebar-container');
        if (container) {
            activeBasketSidebar = new BasketSidebar(container);
        }
    });
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
    let isPolling = false;

    const poll = async () => {
        if (isPolling) {
            console.log("Poll already in progress, skipping...");
            return;
        }

        isPolling = true;
        try {
            statusEl.textContent = "Connecting to Daemon...";
            console.log("Polling daemon via invoke...");

            // Use Tauri invoke to bypass browser security restrictions
            // (fetch from https://tauri.localhost to http://localhost is blocked as mixed content)
            const { invoke } = await import('@tauri-apps/api/core');
            await invoke('rpc_proxy', { method: 'get_daemon_state', params: {} });

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
            }
        } catch (e: any) {
            console.log("Daemon not reachable yet:", e?.message);
            try {
                const { invoke } = await import('@tauri-apps/api/core');
                const sidecarStatus = await invoke('get_sidecar_status');
                statusEl.textContent = `Connecting to Daemon... (sidecar: ${sidecarStatus})`;
            } catch {
                statusEl.textContent = `Connecting to Daemon... (${e?.message || 'connection failed'})`;
            }
        } finally {
            isPolling = false;
        }

        if (Date.now() - startTime > timeout) {
            console.log("Timeout reached");
            if (container) container.classList.add('error');
            try {
                const { invoke } = await import('@tauri-apps/api/core');
                const sidecarStatus = await invoke('get_sidecar_status');
                statusEl.textContent = `Failed to connect to Daemon (sidecar: ${sidecarStatus}).`;
            } catch {
                statusEl.textContent = "Failed to connect to Daemon. Please ensure it is running.";
            }
            return;
        }

        setTimeout(poll, 1000);
    };

    poll();
}

window.addEventListener('DOMContentLoaded', init);
