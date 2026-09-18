import { playbackControl, playbackGetSession, playbackListOutputs, playbackSeek, playbackSelectOutput, serverList, PlaybackOutput, PlaybackSessionSnapshot } from '../rpc';
import { t } from '../i18n';
import { formatServerIdentity } from '../serverIdentity';

export class PlaybackControls {
    private disposed = false;
    private timer: ReturnType<typeof setTimeout> | undefined;
    private repaintTimer: ReturnType<typeof setInterval> | undefined;
    private snapshot: PlaybackSessionSnapshot | undefined;
    private busy = false;
    private outputBusy = false;
    private discovering = false;
    private lastDiscoveryAt = 0;
    private resetOutput = false;
    private outputs: PlaybackOutput[] = [];
    private serverLabels = new Map<string, string>();
    private optionsSignature = '';
    private readonly outputSelect = document.createElement('select');
    private readonly outputStatus = document.createElement('span');
    private readonly outputReset = document.createElement('button');
    private readonly outputDropdown = document.createElement('sl-dropdown');
    private readonly outputToggle = document.createElement('sl-icon-button');
    private commandError = '';
    private scrubPreviewMs: number | undefined;
    private queuedSeek: { positionMs: number; identity: string } | undefined;
    private scrubbing = false;
    private seekBusy = false;
    private anchorPositionMs = 0;
    private anchorAt = 0;
    private readonly timeline = document.createElement('input');
    private readonly elapsed = document.createElement('span');
    private readonly duration = document.createElement('span');
    private readonly seekStatus = document.createElement('span');
    private readonly onPageHide = () => this.destroy();
    private readonly title = document.createElement('strong');
    private readonly status = document.createElement('span');
    private readonly source = document.createElement('span');
    private readonly error = document.createElement('span');
    private readonly primary = this.button('resume');
    private readonly stop = this.button('stop');
    private readonly next = this.button('next');
    private readonly retry = this.button('retry');
    private readonly returnToSession = this.button('returnToSession');
    constructor(private readonly container: HTMLElement) {
        container.className = 'playback-controls';
        container.setAttribute('aria-label', t('playback.controls'));
        const info = document.createElement('div');
        info.className = 'playback-controls__info';
        this.status.className = 'playback-controls__status';
        this.status.setAttribute('role', 'status');
        this.status.setAttribute('aria-live', 'polite');
        this.status.setAttribute('aria-atomic', 'true');
        this.error.className = 'playback-controls__error';
        this.error.setAttribute('role', 'alert');
        const timelineGroup = document.createElement('div');
        timelineGroup.className = 'playback-controls__timeline';
        this.timeline.setAttribute('type', 'range');
        this.timeline.setAttribute('min', '0');
        this.timeline.setAttribute('step', '1000');
        this.timeline.setAttribute('aria-label', t('playback.seek.label'));
        this.timeline.addEventListener('input', () => {
            const value = Number(this.timeline.value);
            if (Number.isSafeInteger(value) && value >= 0) {
                this.scrubbing = true;
                this.scrubPreviewMs = value;
                this.renderTimeline();
            }
        });
        this.timeline.addEventListener('change', () => {
            this.scrubbing = false;
            const value = Number(this.timeline.value);
            if (Number.isSafeInteger(value) && value >= 0) void this.submitSeek(value);
        });
        this.seekStatus.className = 'playback-controls__seek-status';
        this.seekStatus.setAttribute('role', 'status');
        this.seekStatus.setAttribute('aria-live', 'polite');
        timelineGroup.append(this.elapsed, this.timeline, this.duration, this.seekStatus);
        this.source.className = 'playback-controls__source';
        info.append(this.title, this.source, this.status, timelineGroup);
        const outputGroup = document.createElement('div');
        outputGroup.className = 'playback-controls__output';
        this.outputDropdown.className = 'playback-controls__output-dropdown';
        this.outputDropdown.setAttribute('placement', 'bottom-end');
        this.outputDropdown.setAttribute('hoist', '');
        this.outputToggle.setAttribute('slot', 'trigger');
        this.outputToggle.setAttribute('name', 'speaker');
        this.outputToggle.setAttribute('label', t('playback.output.label'));
        const outputLabel = document.createElement('label');
        outputLabel.textContent = t('playback.output.label');
        this.outputSelect.setAttribute('aria-label', t('playback.output.label'));
        outputLabel.append(this.outputSelect);
        this.outputStatus.setAttribute('role', 'status');
        this.outputStatus.setAttribute('aria-live', 'polite');
        const refresh = document.createElement('button');
        refresh.textContent = t('playback.output.refresh');
        refresh.addEventListener('click', () => void this.refreshOutputs());
        this.outputReset.textContent = t('playback.output.reset');
        this.outputReset.addEventListener('click', () => {
            this.resetOutput = true;
            this.outputStatus.textContent = t('playback.output.reset_choose');
            this.outputSelect.focus();
        });
        this.outputReset.hidden = true;
        this.outputSelect.addEventListener('focus', () => void this.refreshOutputs());
        this.outputSelect.addEventListener('blur', () => this.renderOptions());
        this.outputSelect.addEventListener('change', () => void this.chooseOutput());
        outputGroup.append(outputLabel, refresh, this.outputReset, this.outputStatus);
        this.outputDropdown.append(this.outputToggle, outputGroup);
        container.replaceChildren(info, this.primary, this.returnToSession, this.stop, this.next, this.retry, this.outputDropdown, this.error);
        this.primary.hidden = this.returnToSession.hidden = this.stop.hidden = this.next.hidden = this.retry.hidden = true;
        window.addEventListener('pagehide', this.onPageHide, { once: true });
        void this.refreshServerLabels();
        void this.poll();
        this.scheduleRepaint();
    }
    destroy(): void {
        this.disposed = true;
        if (this.timer !== undefined) globalThis.clearTimeout(this.timer);
        if (this.repaintTimer !== undefined) globalThis.clearInterval(this.repaintTimer);
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
                if (this.seekIdentity(previous) !== this.seekIdentity(snapshot)) {
                    this.scrubbing = false;
                    this.scrubPreviewMs = undefined;
                    this.queuedSeek = undefined;
                }
                this.snapshot = snapshot;
                this.anchorPositionMs = snapshot.positionMs;
                this.anchorAt = this.now();
                if (!this.scrubbing && !this.seekBusy && !snapshot.playback.pendingSeek
                    && snapshot.playback.seekOutcome?.status !== 'pending') this.scrubPreviewMs = undefined;
                this.render(snapshot);
                if (!previous || snapshot.instanceId !== previous.instanceId) {
                    this.resetOutput = false;
                    this.outputs = [];
                    void this.refreshOutputs();
                }
            }
        } catch { this.anchorAt = 0; /* daemon lifecycle UI owns connection errors */ }
        if (!this.disposed) {
            // listOutputs returns the owned worker's cached inventory. Pick up
            // its completed refresh through the existing polling lifecycle.
            if (this.snapshot) void this.refreshOutputs(false);
            this.timer = globalThis.setTimeout(() => void this.poll(), 500);
        }
    }
    private render(snapshot: PlaybackSessionSnapshot): void {
        this.title.textContent = snapshot.playback.metadata?.title ?? t('playback.nothing_selected');
        const sourceId = snapshot.current?.source.serverId;
        const sourceLabel = sourceId ? this.serverLabels.get(sourceId) : undefined;
        this.source.textContent = sourceId ? t('playback.source', { source: sourceLabel ?? t('server.default') }) : '';
        this.source.hidden = !sourceId;
        const transportStatus = snapshot.playback.error
            ? t(`playback.error.${snapshot.playback.error.code}`)
            : t(`playback.status.${snapshot.playback.status}`);
        const statusText = snapshot.mode === 'preview'
            ? t('playback.status.preview', { status: transportStatus })
            : transportStatus;
        if (this.status.textContent !== statusText) this.status.textContent = statusText;
        this.error.textContent = this.commandError;
        const action = snapshot.state === 'playing' || snapshot.state === 'buffering' ? 'pause' : 'resume';
        const label = t(action === 'resume' && (snapshot.output?.error?.code ?? snapshot.playback.error?.code) === 'OUTPUT_LOST'
            ? 'playback.resume_selected_output' : `playback.${action}`, { name: snapshot.output?.selected?.displayName ?? '' });
        this.primary.dataset.playbackAction = action;
        this.primary.textContent = label;
        this.primary.setAttribute('aria-label', label);
        this.primary.hidden = this.stop.hidden = !snapshot.current;
        this.primary.disabled = this.stop.disabled = this.busy;
        this.next.hidden = !snapshot.current;
        this.next.disabled = this.busy || !snapshot.playback.canGoNext;
        this.retry.hidden = snapshot.playback.status !== 'error' || !snapshot.playback.error?.retryable;
        this.retry.disabled = this.busy;
        this.returnToSession.hidden = snapshot.mode !== 'preview' || !snapshot.preview?.hasMainSession;
        this.returnToSession.disabled = this.busy;
        if (action === 'resume' && snapshot.output && (!snapshot.output.selected?.available || snapshot.output.pending)) {
            this.primary.disabled = true;
            this.primary.setAttribute('title', t('playback.output.choose'));
        } else { this.primary.setAttribute('title', label); }
        this.outputSelect.disabled = this.outputBusy;
        this.outputReset.hidden = snapshot.output?.error?.code !== 'OUTPUT_CONFIG_INVALID';
        const outputText = snapshot.output?.error
            ? t(`playback.error.${snapshot.output.error.code}`)
            : t(`playback.output.${snapshot.output?.status ?? 'unselected'}`);
        const active = snapshot.output?.active
            ? t('playback.output.active_name', { name: snapshot.output.active.displayName }) : '';
        if (!this.resetOutput) this.outputStatus.textContent = `${outputText} ${active}`.trim();
        this.renderOptions();
        this.renderTimeline();
    }

    private async refreshServerLabels(): Promise<void> {
        try {
            const servers = await serverList();
            if (this.disposed) return;
            this.serverLabels = new Map(servers
                .filter(server => typeof server.serverId === 'string' && server.serverId.length > 0)
                .map(server => [server.serverId as string, formatServerIdentity(server).label]));
            if (this.snapshot) this.render(this.snapshot);
        } catch {
            // Playback remains usable while server configuration is unavailable.
        }
    }

    private scheduleRepaint(): void {
        if (this.disposed || typeof globalThis.setInterval !== 'function') return;
        this.repaintTimer = globalThis.setInterval(() => {
            if (!this.disposed && this.container.isConnected) this.renderTimeline();
            else this.destroy();
        }, 100);
    }

    private shownPositionMs(): number {
        const snapshot = this.snapshot;
        if (!snapshot) return 0;
        let position = this.anchorPositionMs;
        if (snapshot.playback.status === 'active' && !snapshot.playback.pendingSeek && this.anchorAt > 0) {
            const age = Math.max(0, this.now() - this.anchorAt);
            position += Math.min(age, 750);
        }
        const duration = snapshot.playback.durationMs;
        return Math.max(0, duration && duration > 0 ? Math.min(position, duration) : position);
    }

    private renderTimeline(): void {
        const snapshot = this.snapshot;
        const duration = snapshot?.playback.durationMs ?? 0;
        const available = Boolean(snapshot?.current && snapshot.playback.seek?.available && duration > 0);
        const shown = Math.round(this.shownPositionMs());
        this.timeline.max = String(duration > 0 ? duration : 1);
        this.timeline.value = String(Math.min(this.scrubPreviewMs ?? shown, duration > 0 ? duration : 0));
        this.timeline.disabled = !available;
        this.timeline.setAttribute('aria-valuetext', t('playback.seek.value', {
            elapsed: this.formatTime(this.scrubPreviewMs ?? shown), duration: duration > 0 ? this.formatTime(duration) : t('playback.seek.unknown_duration'),
        }));
        this.elapsed.textContent = this.formatTime(shown);
        this.duration.textContent = duration > 0 ? this.formatTime(duration) : t('playback.seek.unknown_duration');
        if (snapshot?.playback.pendingSeek) {
            this.seekStatus.textContent = t('playback.seek.pending');
        } else if (snapshot?.playback.seekOutcome?.status === 'failed') {
            this.seekStatus.textContent = t('playback.seek.failed');
        } else if (!available) {
            this.seekStatus.textContent = t(snapshot?.playback.seek?.reason ?? 'playback.seek.unavailable');
        } else {
            this.seekStatus.textContent = '';
        }
    }

    private formatTime(valueMs: number): string {
        const seconds = Math.max(0, Math.floor(valueMs / 1000));
        return `${Math.floor(seconds / 60)}:${String(seconds % 60).padStart(2, '0')}`;
    }

    private now(): number {
        return globalThis.performance?.now?.() ?? Date.now();
    }

    private seekIdentity(snapshot: PlaybackSessionSnapshot | undefined): string {
        return JSON.stringify([snapshot?.instanceId, snapshot?.sessionId, snapshot?.current?.occurrenceId]);
    }

    private async submitSeek(positionMs: number): Promise<void> {
        if (this.disposed || !this.snapshot || !this.snapshot.playback.seek.available) return;
        const identity = this.seekIdentity(this.snapshot);
        if (this.seekBusy) { this.queuedSeek = { positionMs, identity }; return; }
        this.seekBusy = true;
        this.commandError = '';
        this.render(this.snapshot);
        const observed = this.snapshot;
        try {
            const current = await playbackSeek(positionMs, observed);
            if (!this.disposed && this.seekIdentity(this.snapshot) === identity
                && this.seekIdentity(current) === identity
                && BigInt(current.stateSequence) > BigInt(this.snapshot!.stateSequence)) {
                this.snapshot = current;
                this.anchorPositionMs = current.positionMs;
                this.anchorAt = this.now();
            }
        } catch (error) {
            if (this.seekIdentity(this.snapshot) === identity) {
                const code = (error as { data?: { code?: string } })?.data?.code;
                this.commandError = code ? t(`playback.error.${code}`) : t('playback.command_error');
                this.scrubPreviewMs = undefined;
            }
        } finally {
            this.seekBusy = false;
            const queued = this.queuedSeek;
            this.queuedSeek = undefined;
            if (queued && queued.identity === this.seekIdentity(this.snapshot)
                && queued.positionMs !== positionMs) {
                void this.submitSeek(queued.positionMs);
            } else if (!this.disposed && this.snapshot) {
                if (!this.scrubbing && !this.snapshot.playback.pendingSeek) this.scrubPreviewMs = undefined;
                this.render(this.snapshot);
            }
        }
    }

    private renderOptions(): void {
        const focusedValue = document.activeElement === this.outputSelect ? this.outputSelect.value : undefined;
        const selected = this.snapshot?.output?.selected;
        const pending = this.snapshot?.output?.pending;
        const choices = [...this.outputs];
        if (selected && !choices.some(o => o.outputId === selected.outputId)) choices.push({ ...selected, available: false });
        const signature = JSON.stringify(choices);
        if (signature !== this.optionsSignature) {
            const existing = new Map(Array.from(this.outputSelect.children).map(child => {
                const option = child as HTMLOptionElement;
                return [option.value, option] as const;
            }));
            const placeholder = existing.get('') ?? document.createElement('option');
            placeholder.value = ''; placeholder.textContent = t('playback.output.choose');
            placeholder.disabled = true;
            const options = choices.map(output => {
                const option = existing.get(output.outputId) ?? document.createElement('option');
                option.value = output.outputId;
                option.textContent = [output.displayName, output.detail,
                    output.isDefault ? t('playback.output.default') : '',
                    !output.available ? t('playback.output.unavailable') : '',
                    output.identityConfidence === 'unsupported' ? t('playback.error.OUTPUT_SHARED_UNSUPPORTED') : '',
                    output.identityConfidence === 'fallback' ? t('playback.output.fallback') : '',
                    output.isVirtual ? t('playback.output.virtual') : ''].filter(Boolean).join(' · ');
                option.disabled = !output.available;
                return option;
            });
            const retained = new Set([placeholder, ...options]);
            for (const option of existing.values()) if (!retained.has(option)) option.remove();
            // Retain the select and existing option nodes while reconciling a
            // focus-triggered inventory response, including newly attached devices.
            for (const option of retained) this.outputSelect.append(option);
            this.optionsSignature = signature;
        }
        this.outputSelect.value = focusedValue && choices.some(o => o.outputId === focusedValue)
            ? focusedValue : pending?.outputId ?? selected?.outputId ?? '';
    }

    private async refreshOutputs(force = true): Promise<void> {
        if (this.disposed || this.discovering) return;
        if (!force && Date.now() - this.lastDiscoveryAt < 1000) return;
        this.lastDiscoveryAt = Date.now();
        this.discovering = true;
        const instance = this.snapshot?.instanceId;
        try {
            const result = await playbackListOutputs();
            if (this.disposed || result.instanceId !== this.snapshot?.instanceId || instance !== this.snapshot?.instanceId) return;
            this.outputs = result.outputs;
            this.renderOptions();
            if (result.error) this.outputStatus.textContent = t(`playback.error.${result.error.code}`);
        } catch { /* session polling retains the last authoritative selection */ }
        finally { this.discovering = false; }
    }

    private async chooseOutput(): Promise<void> {
        if (this.disposed || this.outputBusy || !this.snapshot || !this.outputSelect.value) return;
        const observed = this.snapshot;
        const outputId = this.outputSelect.value;
        this.outputBusy = true;
        this.commandError = '';
        this.render(observed);
        try {
            const current = await playbackSelectOutput(outputId, observed, this.resetOutput);
            if (!this.disposed && this.snapshot?.instanceId === observed.instanceId) {
                if (BigInt(current.stateSequence) >= BigInt(this.snapshot.stateSequence)) this.snapshot = current;
                this.resetOutput = false;
            }
        } catch (error) {
            const code = (error as { data?: { code?: string } })?.data?.code;
            this.commandError = code?.startsWith('OUTPUT_') ? t(`playback.error.${code}`) : t('playback.command_error');
            try {
                const current = await playbackGetSession();
                if (!this.disposed && current.instanceId === observed.instanceId) this.snapshot = current;
            } catch { /* preserve the structured command failure */ }
        } finally {
            this.outputBusy = false;
            if (!this.disposed && this.snapshot) this.render(this.snapshot);
        }
    }
    private button(action: 'pause' | 'resume' | 'stop' | 'next' | 'retry' | 'returnToSession'): HTMLElement & { disabled: boolean } {
        const button = document.createElement('sl-button') as any;
        button.size = 'small';
        button.dataset.playbackAction = action;
        const labelKey = action === 'returnToSession' ? 'playback.return_to_session' : `playback.${action}`;
        button.textContent = t(labelKey);
        button.setAttribute('aria-label', t(labelKey));
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
