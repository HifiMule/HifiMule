import { playbackControl, playbackGetSession, PlaybackSessionSnapshot } from '../rpc';
import { t } from '../i18n';

export class PlaybackControls {
    private disposed = false;
    private timer: ReturnType<typeof setTimeout> | undefined;
    private snapshot: PlaybackSessionSnapshot | undefined;
    private busy = false;
    private commandError = '';
    private readonly onPageHide = () => this.destroy();
    private readonly title = document.createElement('strong');
    private readonly status = document.createElement('span');
    private readonly error = document.createElement('span');
    private readonly primary = this.button('resume');
    private readonly stop = this.button('stop');
    constructor(private readonly container: HTMLElement) {
        container.className = 'playback-controls';
        container.setAttribute('aria-label', t('playback.controls'));
        const info = document.createElement('div');
        info.className = 'playback-controls__info';
        this.status.className = 'playback-controls__status';
        this.status.setAttribute('role', 'status');
        this.status.setAttribute('aria-live', 'polite');
        this.status.setAttribute('aria-atomic', 'true');
        this.error.setAttribute('role', 'alert');
        info.append(this.title, this.status, this.error);
        container.replaceChildren(info, this.primary, this.stop);
        this.primary.hidden = this.stop.hidden = true;
        window.addEventListener('pagehide', this.onPageHide, { once: true });
        void this.poll();
    }
    destroy(): void {
        this.disposed = true;
        if (this.timer !== undefined) globalThis.clearTimeout(this.timer);
        window.removeEventListener('pagehide', this.onPageHide);
    }
    private async poll(): Promise<void> {
        if (this.disposed || !this.container.isConnected) { this.destroy(); return; }
        try {
            const snapshot = await playbackGetSession();
            const previous = this.snapshot;
            if (!this.disposed && (!previous || snapshot.instanceId !== previous.instanceId
                || snapshot.sessionId !== previous.sessionId
                || BigInt(snapshot.stateSequence) > BigInt(previous.stateSequence))) {
                this.snapshot = snapshot;
                this.render(snapshot);
            }
        } catch { /* daemon lifecycle UI owns connection errors */ }
        if (!this.disposed) this.timer = globalThis.setTimeout(() => void this.poll(), 500);
    }
    private render(snapshot: PlaybackSessionSnapshot): void {
        this.title.textContent = snapshot.playback.metadata?.title ?? t('playback.nothing_selected');
        const statusText = snapshot.playback.error
            ? t(`playback.error.${snapshot.playback.error.code}`)
            : t(`playback.status.${snapshot.playback.status}`);
        if (this.status.textContent !== statusText) this.status.textContent = statusText;
        this.error.textContent = this.commandError;
        const action = snapshot.state === 'playing' || snapshot.state === 'buffering' ? 'pause' : 'resume';
        const label = t(action === 'resume' && snapshot.playback.error?.code === 'OUTPUT_LOST'
            ? 'playback.resume_default_output' : `playback.${action}`);
        this.primary.dataset.playbackAction = action;
        this.primary.textContent = label;
        this.primary.setAttribute('aria-label', label);
        this.primary.hidden = this.stop.hidden = !snapshot.current;
        this.primary.disabled = this.stop.disabled = this.busy;
    }
    private button(action: 'pause' | 'resume' | 'stop'): HTMLElement & { disabled: boolean } {
        const button = document.createElement('sl-button') as any;
        button.size = 'small';
        button.dataset.playbackAction = action;
        button.textContent = t(`playback.${action}`);
        button.setAttribute('aria-label', t(`playback.${action}`));
        button.addEventListener('click', async () => {
            if (this.busy || this.disposed || !this.snapshot) return;
            this.busy = true;
            this.commandError = '';
            const selected = this.snapshot;
            this.render(selected);
            try {
                await playbackControl(button.dataset.playbackAction, selected);
            } catch (error) {
                const code = (error as { data?: { code?: string } })?.data?.code;
                this.commandError = t(code === 'PERSISTENCE_FAILED'
                    ? 'playback.command_error.persistence' : 'playback.command_error');
            } finally {
                this.busy = false;
                if (!this.disposed && this.snapshot) this.render(this.snapshot);
            }
        });
        return button;
    }
}
