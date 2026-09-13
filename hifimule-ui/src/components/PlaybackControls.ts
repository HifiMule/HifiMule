import { playbackControl, playbackGetSession, PlaybackSessionSnapshot } from '../rpc';
import { t } from '../i18n';

export class PlaybackControls {
    private disposed = false;
    private timer: ReturnType<typeof setTimeout> | undefined;
    private lastSequence = '';
    constructor(private readonly container: HTMLElement) {
        window.addEventListener('pagehide', () => this.destroy(), { once: true });
        void this.poll();
    }
    destroy(): void {
        this.disposed = true;
        if (this.timer !== undefined) globalThis.clearTimeout(this.timer);
    }
    private async poll(): Promise<void> {
        try {
            const snapshot = await playbackGetSession();
            if (!this.disposed && snapshot.stateSequence !== this.lastSequence) {
                this.lastSequence = snapshot.stateSequence;
                this.render(snapshot);
            }
        } catch { /* daemon lifecycle UI owns connection errors */ }
        if (!this.disposed) this.timer = globalThis.setTimeout(() => void this.poll(), 500);
    }
    private render(snapshot: PlaybackSessionSnapshot): void {
        const focused = (document.activeElement as HTMLElement | null)?.dataset.playbackAction;
        this.container.replaceChildren();
        this.container.className = 'playback-controls';
        this.container.setAttribute('aria-label', t('playback.controls'));
        const info = document.createElement('div');
        info.className = 'playback-controls__info';
        const title = document.createElement('strong');
        title.textContent = snapshot.playback.metadata?.title ?? t('playback.nothing_selected');
        const status = document.createElement('span');
        status.className = 'playback-controls__status';
        status.setAttribute('aria-live', 'polite');
        status.textContent = snapshot.playback.error
            ? t(`playback.error.${snapshot.playback.error.code}`)
            : t(`playback.status.${snapshot.playback.status}`);
        info.append(title, status);
        this.container.appendChild(info);
        if (snapshot.current) {
            const primary = snapshot.playback.status === 'active' ? 'pause' : 'resume';
            this.container.append(this.button(primary), this.button('stop'));
        }
        if (focused) this.container.querySelector<HTMLElement>(`[data-playback-action="${focused}"]`)?.focus();
    }
    private button(action: 'pause' | 'resume' | 'stop'): HTMLElement {
        const button = document.createElement('sl-button') as any;
        button.size = 'small';
        button.dataset.playbackAction = action;
        button.textContent = t(`playback.${action}`);
        button.addEventListener('click', async () => {
            button.disabled = true;
            try { await playbackControl(action); } finally { button.disabled = false; }
        });
        return button;
    }
}
