import '@shoelace-style/shoelace/dist/themes/dark.css';
import '@shoelace-style/shoelace/dist/shoelace.js';
import { setBasePath } from '@shoelace-style/shoelace/dist/utilities/base-path.js';
import { LogicalSize } from '@tauri-apps/api/dpi';
import { Window, currentMonitor } from '@tauri-apps/api/window';
import { t } from './i18n';
import { withDeadline } from './lifecycleDeadline';
import { shutdownMessageKey, ShutdownPoller, canRetryQuit } from './shutdownStatus';

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
        await waitForNativeReadiness();
        const state = await waitForDaemonState(rpcCall);
        await routeFromDaemonState(state);
        await showMainWindow();
        const { invoke } = await import('@tauri-apps/api/core');
        await invoke('report_ui_ready');
        observeShutdown(rpcCall);
    } catch (e) {
        console.error("Failed to check daemon state", e);
        const raw = e instanceof Error ? e.message : String(e);
        if (raw.startsWith('DAEMON_STOPPED')) {
            try {
                const health = await rpcCall('daemon.health');
                if (health?.data?.shutdown) {
                    renderShutdownStatus(rpcCall, health.data);
                    const { invoke } = await import('@tauri-apps/api/core');
                    await invoke('report_shutdown_rendered', {
                        shutdownId: health.data.shutdown.shutdownId,
                    });
                } else {
                    renderLifecycleFailure(e);
                }
            } catch {
                renderLifecycleFailure(e);
            }
        } else {
            renderLifecycleFailure(e);
        }
        await showMainWindow();
    }
}

function observeShutdown(rpcCall: (method: string, params?: any) => Promise<any>): void {
    let disposed = false;
    const poller = new ShutdownPoller(async () => {
        try {
            const health = await rpcCall('daemon.health');
            if (!disposed && health?.data?.shutdown
                && !(canRetryQuit(health.data) && sessionStorage.getItem("dismissedQuit") === health.data.shutdown.shutdownId)) {
                poller.dispose();
                renderShutdownStatus(rpcCall, health.data);
                if (health.data.status === 'stopping') {
                    const { invoke } = await import('@tauri-apps/api/core');
                    await invoke('report_shutdown_rendered', {
                        shutdownId: health.data.shutdown.shutdownId,
                    });
                }
                disposed = true;
            }
        } catch {
            // A transient health failure is unknown, not proof of clean exit.
        }
    });
    window.addEventListener('pagehide', () => {
        disposed = true;
        poller.dispose();
    }, { once: true });
}

type ShutdownHealth = {
    status: string;
    errorCode?: string | null;
    shutdown?: {
        shutdownId: string;
        phase: string;
        elapsedMs: number;
        deadlineExceeded: boolean;
        activeOperationCount: number;
        pendingMutationCount: number;
    } | null;
};

function renderShutdownStatus(
    rpcCall: (method: string, params?: any) => Promise<any>,
    initial: ShutdownHealth,
): void {
    activeBasketSidebar?.destroy();
    activeBasketSidebar = null;
    document.body.innerHTML = `
        <main class="login-container" aria-labelledby="shutdown-title">
            <section class="login-card" style="padding:2rem;max-width:40rem">
                <h2 id="shutdown-title">${t('lifecycle.quitting_waiting')}</h2>
                <p id="shutdown-status" role="status" aria-live="polite"></p>
                <div style="display:flex;gap:.75rem">
                    <button id="shutdown-retry" type="button" hidden>${t('lifecycle.retry_quit')}</button>
                    <button id="shutdown-continue" type="button" hidden>${t('lifecycle.continue_running')}</button>
                    <button id="shutdown-refresh" type="button">${t('lifecycle.refresh')}</button>
                    <button id="shutdown-close" type="button">${t('lifecycle.close')}</button>
                </div>
            </section>
        </main>`;
    const status = document.getElementById('shutdown-status');
    const refresh = document.getElementById('shutdown-refresh') as HTMLButtonElement | null;
    let disposed = false;
    const retry = document.getElementById('shutdown-retry') as HTMLButtonElement;
    const resume = document.getElementById('shutdown-continue') as HTMLButtonElement;
    let current = initial;
    const update = (health: ShutdownHealth) => {
        if (disposed) return;
        current = health;
        retry.hidden = resume.hidden = !canRetryQuit(health);
        const shutdown = health.shutdown;
        if (!status || !shutdown) return;
        const messageKey = shutdownMessageKey(shutdown, health.errorCode);
        status.textContent = messageKey !== 'lifecycle.shutdown_progress'
            ? t(messageKey)
            : t(messageKey, {
                operations: String(shutdown.activeOperationCount),
                mutations: String(shutdown.pendingMutationCount),
            });
        document.body.dataset.shutdownId = shutdown.shutdownId;
        document.body.dataset.shutdownPhase = shutdown.phase;
    };
    const poller = new ShutdownPoller(async () => {
        if (disposed) return;
        try {
            const result = await rpcCall('daemon.health');
            update(result.data);
        } catch {
            if (!disposed && status) status.textContent = t('lifecycle.shutdown_unreachable');
        }
    });
    update(initial);
    refresh?.addEventListener('click', () => poller.refresh());
    retry.addEventListener('click', async () => {
        if (!canRetryQuit(current) || retry.disabled) return;
        retry.disabled = true;
        resume.disabled = true;
        try {
            await rpcCall('daemon.retryQuit');
            retry.hidden = resume.hidden = true;
            poller.refresh();
        } catch {
            if (!disposed && status) status.textContent = t('lifecycle.quit_persistence_failed');
        } finally { retry.disabled = resume.disabled = false; }
    });
    resume.addEventListener('click', () => {
        if (!canRetryQuit(current)) return;
        sessionStorage.setItem('dismissedQuit', current.shutdown!.shutdownId);
        disposed = true;
        poller.dispose();
        window.location.reload();
    });
    refresh?.focus();
    document.getElementById('shutdown-close')?.addEventListener('click', () => {
        void import('@tauri-apps/api/core').then(({ invoke }) => invoke('close_ui'));
    });
    window.addEventListener('pagehide', () => {
        disposed = true;
        poller.dispose();
    }, { once: true });
}

async function showMainWindow(): Promise<void> {
    await (await Window.getByLabel('main'))?.show();
    await (await Window.getByLabel('splashscreen'))?.close();
}

async function waitForNativeReadiness(): Promise<void> {
    const { invoke } = await import('@tauri-apps/api/core');
    const deadline = performance.now() + 15_000;
    while (performance.now() < deadline) {
        const status = await invoke<{ state: string; errorCode?: string }>('get_sidecar_status');
        if (status.state === 'ready') return;
        if (['failed', 'stopping', 'stopped'].includes(status.state)) {
            throw new Error(status.errorCode ?? 'DAEMON_STOPPED');
        }
        await new Promise(resolve => setTimeout(resolve, 250));
    }
    throw new Error('STARTUP_TIMEOUT');
}

async function waitForDaemonState(rpcCall: (method: string, params?: any) => Promise<any>): Promise<any> {
    return withDeadline(rpcCall('get_daemon_state'), 15_000, 'STATE_LOAD_TIMEOUT');
}

function renderLifecycleFailure(error: unknown): void {
    const raw = error instanceof Error ? error.message : String(error);
    const code = raw.split(':', 1)[0];
    const knownCodes = ['LEGACY_DAEMON_RUNNING', 'LEGACY_ENDPOINT_OCCUPIED', 'LOCAL_ACCESS_DENIED',
        'PROTOCOL_MISMATCH', 'OWNER_CHANGED', 'DAEMON_STOPPED', 'SPAWN_FAILED', 'STARTUP_TIMEOUT',
        'STATE_LOAD_TIMEOUT', 'UNSAFE_RUNTIME_PATH'];
    const message = knownCodes.includes(code) ? t(`lifecycle.error.${code}`) : raw;
    document.body.innerHTML = `
        <main class="login-container" role="alert" aria-live="assertive">
            <section class="login-card" style="padding:2rem;max-width:36rem">
                <h2>${t('lifecycle.startup_failed_title')}</h2>
                <p>${t('lifecycle.startup_failed_body')}</p>
                <p class="error-text">${escapeLifecycleText(message)}</p>
                <div style="display:flex;gap:.75rem">
                    <button id="lifecycle-retry" type="button">${t('lifecycle.retry')}</button>
                    <button id="lifecycle-close" type="button">${t('lifecycle.close')}</button>
                </div>
            </section>
        </main>`;
    const retry = document.getElementById('lifecycle-retry') as HTMLButtonElement | null;
    retry?.focus();
    retry?.addEventListener('click', async () => {
        retry.disabled = true;
        const { invoke } = await import('@tauri-apps/api/core');
        await invoke('retry_daemon_startup');
        window.location.reload();
    });
    document.getElementById('lifecycle-close')?.addEventListener('click', () => {
        void import('@tauri-apps/api/core').then(({ invoke }) => invoke('close_ui'));
    });
}

function escapeLifecycleText(value: string): string {
    const element = document.createElement('span');
    element.textContent = value;
    return element.innerHTML;
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
    const retryButton = document.getElementById('retry-btn');
    const closeButton = document.getElementById('close-btn');
    if (retryButton) retryButton.textContent = t('lifecycle.retry');
    if (closeButton) closeButton.textContent = t('lifecycle.close');
    document.getElementById('close-btn')?.addEventListener('click', () => {
        void import('@tauri-apps/api/core').then(({ invoke }) => invoke('close_ui'));
    });

    if (!statusEl) {
        console.error("Status element not found!");
        return;
    }

    // The main webview alone hydrates and changes routes. Splash reflects the same
    // native attempt and is closed by main only after routing (or rendering failure).
    const { invoke } = await import('@tauri-apps/api/core');
    let disposed = false;
    let timer: ReturnType<typeof setTimeout> | undefined;
    window.addEventListener('pagehide', () => {
        disposed = true;
        if (timer !== undefined) clearTimeout(timer);
    }, { once: true });
    retryButton?.addEventListener('click', async () => {
        await invoke('retry_daemon_startup');
        // Restart the main webview too; it owns state hydration.
        await invoke('reload_main_window');
        window.location.reload();
    });
    const poll = async () => {
        try {
            const status = await invoke<{state: string; errorCode?: string}>('get_sidecar_status');
            if (disposed) return;
            statusEl.textContent = status.state === 'ready'
                ? t('ui.splash.daemon_ready')
                : status.errorCode ?? t('ui.splash.connecting_daemon');
            if (['failed', 'stopped'].includes(status.state)) {
                container?.classList.add('error');
                (retryButton as HTMLButtonElement | null)?.focus();
                return;
            }
        } catch {
            if (disposed) return;
            statusEl.textContent = t('ui.splash.failed');
        }
        if (!disposed) timer = setTimeout(poll, 250);
    };
    void mainWin;
    void splashWin;
    void poll();
}

window.addEventListener('DOMContentLoaded', init);
