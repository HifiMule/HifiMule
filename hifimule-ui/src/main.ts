import { LogicalSize } from '@tauri-apps/api/dpi';
import { Window, currentMonitor } from '@tauri-apps/api/window';
import { t } from './i18n';

// HifiMule UI Main Entry Point
// Coordinates splash screen and main window lifecycle.

let activeBasketSidebar: any = null;

const IDEAL_MAIN_WIDTH = 1280;
const IDEAL_MAIN_HEIGHT = 860;
const COMFORT_MIN_MAIN_WIDTH = 1040;
const COMFORT_MIN_MAIN_HEIGHT = 720;
const ABSOLUTE_MIN_MAIN_WIDTH = 900;
const ABSOLUTE_MIN_MAIN_HEIGHT = 640;

type CurrentServer = {
    serverId: string;
    url: string;
    username: string;
    serverType: string;
    serverVersion?: string | null;
} | null;

function serverTypeLabel(type?: string | null): string {
    switch (type) {
        case 'jellyfin': return 'Jellyfin';
        case 'openSubsonic': return 'OpenSubsonic';
        case 'subsonic': return 'Subsonic';
        default: return t('server.default');
    }
}

function serverBadgeVariant(type?: string | null): string {
    switch (type) {
        case 'jellyfin': return 'primary';
        case 'openSubsonic': return 'success';
        case 'subsonic': return 'neutral';
        default: return 'neutral';
    }
}

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

    const { rpcCall } = await import('./rpc');

    try {
        const state = await rpcCall('get_daemon_state');
        renderMainLayout(state.currentServer ?? null);

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
                rpcCall('get_daemon_state').then((newState) => {
                    renderMainLayout(newState.currentServer ?? null);
                    import('./library').then(({ initLibraryView }) => initLibraryView());
                });
            });
        }
    } catch (e) {
        console.error("Failed to check daemon state", e);
        renderMainLayout(null);
        // Fallback to login
        const { initLoginView } = await import('./login');
        initLoginView(() => {
            rpcCall('get_daemon_state').then((newState) => {
                renderMainLayout(newState.currentServer ?? null);
                import('./library').then(({ initLibraryView }) => initLibraryView());
            });
        });
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

function renderMainLayout(currentServer: CurrentServer = null) {
    const root = document.querySelector('.app-container');
    if (!root) return;
    import('./state/basket').then(({ basketStore }) => {
        basketStore.setActiveServerId(currentServer?.serverId ?? null);
    });

    const serverLabel = serverTypeLabel(currentServer?.serverType);
    const serverVersion = currentServer?.serverVersion ? ` ${currentServer.serverVersion}` : '';
    const serverHint = currentServer
        ? `
            <div class="server-connection-chip">
              <sl-badge variant="${serverBadgeVariant(currentServer.serverType)}" pill>${serverLabel}</sl-badge>
              <span title="${currentServer.url}">${currentServer.username} @ ${currentServer.url}${serverVersion}</span>
              <sl-icon-button id="logout-btn" name="box-arrow-right" label="${t('ui.logout')}"></sl-icon-button>
            </div>
        `
        : '';

    root.innerHTML = `
    <sl-split-panel position="68" class="split-panel">
      <div slot="start" class="library-view">
        <header>
          <div class="library-header-row">
            <div class="library-title-block">
              <h1>${t('ui.library.title')}</h1>
              <p>${t('ui.library.subtitle')}</p>
            </div>
            ${serverHint}
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

    document.getElementById('logout-btn')?.addEventListener('click', async () => {
        const { rpcCall } = await import('./rpc');
        const { basketStore } = await import('./state/basket');
        try {
            await rpcCall('server.logout');
            basketStore.setActiveServerId(null);
            basketStore.clearLocalOnly();
            if (activeBasketSidebar) {
                activeBasketSidebar.destroy();
                activeBasketSidebar = null;
            }
            const { initLoginView } = await import('./login');
            initLoginView(() => {
                rpcCall('get_daemon_state').then((newState) => {
                    renderMainLayout(newState.currentServer ?? null);
                    import('./library').then(({ initLibraryView }) => initLibraryView());
                });
            });
        } catch (error) {
            console.error('Logout failed', error);
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
