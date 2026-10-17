import { t, initializeI18n } from './i18n';

// Keep startup status independent of the main application's UI components.
async function initSplashScreen() {
    await initializeI18n();
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
    void poll();
}

window.addEventListener('DOMContentLoaded', initSplashScreen);
