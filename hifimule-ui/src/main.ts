import '@shoelace-style/shoelace/dist/themes/dark.css';
import '@shoelace-style/shoelace/dist/shoelace.js';
import { setBasePath } from '@shoelace-style/shoelace/dist/utilities/base-path.js';
import { LogicalSize } from '@tauri-apps/api/dpi';
import { Window, currentMonitor } from '@tauri-apps/api/window';
import { t } from './i18n';

const isDev = Boolean((import.meta as any).env?.DEV);
setBasePath(new URL(isDev
    ? '../node_modules/@shoelace-style/shoelace/dist'
    : '../shoelace', import.meta.url).href);

// HifiMule UI Main Entry Point
// Coordinates splash screen and main window lifecycle.

let activeBasketSidebar: any = null;

const IDEAL_MAIN_WIDTH = 1280;
const IDEAL_MAIN_HEIGHT = 860;
const COMFORT_MIN_MAIN_WIDTH = 1040;
const COMFORT_MIN_MAIN_HEIGHT = 720;
const ABSOLUTE_MIN_MAIN_WIDTH = 900;
const ABSOLUTE_MIN_MAIN_HEIGHT = 640;

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
    await fitMainWindowToMonitor();

    // AC11: a browse RPC hitting an expired/invalid credential surfaces a re-auth
    // prompt scoped to the selected server's URL.
    registerReauthHandler();

    const { rpcCall } = await import('./rpc');

    try {
        const state = await rpcCall('get_daemon_state');
        await routeFromDaemonState(state);
    } catch (e) {
        console.error("Failed to check daemon state", e);
        // Fallback to first-run login.
        const { initLoginView } = await import('./login');
        initLoginView(() => { reloadFromDaemon(); });
    }
}

/**
 * Drives the top-level UI mode from the multi-server daemon state (Story 2.11 AC10):
 *   - 0 servers configured        → full-screen first-run login (Story 2.5)
 *   - ≥1 server, none selected    → main layout with the AC9 in-app empty state
 *   - a server selected           → main layout + library
 */
async function routeFromDaemonState(state: any): Promise<void> {
    const servers: any[] = state?.servers ?? [];
    const selectedServerId: string | null = state?.selectedServerId ?? null;
    // Story 2.13: the basket's active-server key is the PORTABLE id, not the
    // machine-local id, so a single-server user's own items never render locked
    // and newly-tagged items carry the portable identity end-to-end (AC10).
    const selectedServerPortableId: string | null = state?.selectedServerPortableId ?? null;

    if (servers.length === 0) {
        const { initLoginView } = await import('./login');
        initLoginView(() => { reloadFromDaemon(); });
        return;
    }

    renderMainLayout(state);

    const { basketStore } = await import('./state/basket');
    basketStore.setActiveServerId(selectedServerPortableId);

    if (selectedServerId) {
        const { initLibraryView } = await import('./library');
        initLibraryView();
    } else {
        renderLibraryNoServerSelected();
    }
}

/** Re-fetches daemon state and re-routes (after login/select/remove/logout). */
async function reloadFromDaemon(): Promise<void> {
    const { rpcCall } = await import('./rpc');
    try {
        const state = await rpcCall('get_daemon_state');
        await routeFromDaemonState(state);
    } catch (e) {
        console.error('Failed to reload daemon state', e);
    }
}

let reauthInFlight = false;

/** AC11: shows a re-auth dialog scoped to the selected server when a browse RPC
 * reports an expired/invalid credential. Registered once; debounced so repeated
 * 401s don't stack dialogs. */
function registerReauthHandler(): void {
    window.addEventListener('hifimule:server-unauthorized', async () => {
        if (reauthInFlight) return;
        reauthInFlight = true;
        try {
            const { rpcCall } = await import('./rpc');
            const state = await rpcCall('get_daemon_state');
            const url: string | undefined = state?.currentServer?.url;
            if (!url) {
                reauthInFlight = false;
                return;
            }
            const { initLoginView } = await import('./login');
            initLoginView(
                () => { reloadFromDaemon(); },
                {
                    mode: 'reauth',
                    prefillUrl: url,
                    onClose: () => { reauthInFlight = false; },
                }
            );
        } catch (e) {
            console.error('Re-auth prompt failed', e);
            reauthInFlight = false;
        }
    });
}

/** AC9: servers exist but none selected — prompt the user to pick one. */
function renderLibraryNoServerSelected(): void {
    const content = document.getElementById('library-content');
    if (content) {
        content.innerHTML = `
            <div class="library-empty-state" style="padding: 2rem; text-align: center; opacity: 0.7;">
                <sl-icon name="hdd-network" style="font-size: 2rem;"></sl-icon>
                <p>${t('library.selectServerEmpty')}</p>
            </div>
        `;
    }
}

async function fitMainWindowToMonitor() {
    try {
        const appWindow = Window.getCurrent();
        const monitor = await currentMonitor();
        const scaleFactor = monitor?.scaleFactor || await appWindow.scaleFactor();
        const workArea = monitor?.workArea?.size?.toLogical(scaleFactor);
        if (!workArea) return;

        const availableWidth = Math.floor(workArea.width * 0.92);
        const availableHeight = Math.floor(workArea.height * 0.9);
        const minWidth = availableWidth < ABSOLUTE_MIN_MAIN_WIDTH
            ? availableWidth
            : Math.min(COMFORT_MIN_MAIN_WIDTH, availableWidth);
        const minHeight = availableHeight < ABSOLUTE_MIN_MAIN_HEIGHT
            ? availableHeight
            : Math.min(COMFORT_MIN_MAIN_HEIGHT, availableHeight);
        const targetWidth = Math.max(
            minWidth,
            Math.min(IDEAL_MAIN_WIDTH, availableWidth),
        );
        const targetHeight = Math.max(
            minHeight,
            Math.min(IDEAL_MAIN_HEIGHT, availableHeight),
        );

        await appWindow.setMinSize(new LogicalSize(minWidth, minHeight));
        await appWindow.setSize(new LogicalSize(targetWidth, targetHeight));
        await appWindow.center();
    } catch (error) {
        console.warn('Unable to fit main window to monitor:', error);
    }
}

function renderMainLayout(_state: any = null) {
    const root = document.querySelector('.app-container');
    if (!root) return;

    // Avoid rebuilding the whole layout (and tearing down the Server Hub /
    // BasketSidebar) on every reload — only build once. Guard on a marker unique
    // to the *real* layout (`#server-hub-container`), NOT `.split-panel`, because
    // index.html ships a static `.split-panel` placeholder that must be replaced
    // on first render.
    if (root.querySelector('#server-hub-container')) return;

    root.innerHTML = `
    <sl-split-panel primary="end" position="32" class="split-panel">
      <div slot="start" class="library-view">
        <header>
          <div class="library-header-row">
            <div class="library-title-block">
              <h1>${t('ui.library.title')}</h1>
              <p>${t('ui.library.subtitle')}</p>
            </div>
            <div id="server-hub-container"></div>
          </div>
        </header>

        <div id="browse-mode-bar"></div>

        <div id="library-content" class="content">
          <!-- Media grid will be rendered here by library.ts -->
        </div>
      </div>

      <div slot="end" class="basket-view" id="basket-sidebar-container">
        <!-- BasketSidebar component will render here -->
      </div>
    </sl-split-panel>
    `;

    // Mount the Server Hub (list / switch / add / remove / logout). On any change
    // it re-routes the whole UI from fresh daemon state.
    import('./components/ServerHub').then(({ ServerHub }) => {
        const container = document.getElementById('server-hub-container');
        if (container) {
            // The instance stays reachable via its DOM event listeners.
            new ServerHub(container, () => { reloadFromDaemon(); });
        }
    });

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
            statusEl.textContent = t('ui.splash.connecting_daemon');
            console.log("Polling daemon via invoke...");

            // Use Tauri invoke to bypass browser security restrictions
            // (fetch from https://tauri.localhost to http://localhost is blocked as mixed content)
            const { invoke } = await import('@tauri-apps/api/core');
            await invoke('rpc_proxy', { method: 'get_daemon_state', params: {} });

            console.log("Daemon responded!");
            statusEl.textContent = t('ui.splash.daemon_ready');

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
                statusEl.textContent = t('ui.splash.ui_api_error');
            }
        } catch (e: any) {
            console.log("Daemon not reachable yet:", e?.message);
            try {
                const { invoke } = await import('@tauri-apps/api/core');
                const sidecarStatus = await invoke('get_sidecar_status');
                statusEl.textContent = t('ui.splash.connecting_daemon_sidecar', { status: String(sidecarStatus) });
            } catch {
                statusEl.textContent = t('ui.splash.connecting_daemon_error', {
                    error: e?.message || t('ui.splash.connection_failed')
                });
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
                statusEl.textContent = t('ui.splash.failed_sidecar', { status: String(sidecarStatus) });
            } catch {
                statusEl.textContent = t('ui.splash.failed');
            }
            return;
        }

        setTimeout(poll, 1000);
    };

    poll();
}

window.addEventListener('DOMContentLoaded', init);
