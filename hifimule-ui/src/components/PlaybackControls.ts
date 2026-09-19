import { playbackControl, playbackDescribeOccurrences, OccurrenceDisplay, playbackListOutputs, playbackSeek, playbackSelectOutput, serverList, PlaybackOutput, PlaybackSessionSnapshot } from '../rpc';
import { t } from '../i18n';
import { formatServerIdentity } from '../serverIdentity';
import { playbackStore } from '../state/playback';

export class PlaybackControls {
    private disposed = false;
    private unsubscribePlayback?: () => void;
    private unsubscribeConnection?: () => void;
    private interactionEpoch = 0;
    private resizeObserver?: ResizeObserver;
    private repaintTimer: ReturnType<typeof setInterval> | undefined;
    private snapshot: PlaybackSessionSnapshot | undefined;
    private busy = false;
    private outputBusy = false;
    private discovering = false;
    private lastDiscoveryAt = 0;
    private resetOutput = false;
    private outputs: PlaybackOutput[] = [];
    private serverIdentities = new Map<string, { label: string; icon: string }>();
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
    private readonly source = document.createElement('sl-icon');
    private readonly sourceHint = document.createElement('sl-tooltip');
    private readonly error = document.createElement('span');
    private readonly messages = document.createElement('div');
    private statusPopup = true;
    private readonly primary = this.button('resume');
    private readonly stop = this.button('stop');
    private readonly next = this.button('next');
    private readonly retry = this.button('retry');
    private readonly returnToSession = this.button('returnToSession');
    private readonly artist = document.createElement('span');
    private readonly refresh = document.createElement('sl-button') as HTMLElement & { disabled: boolean };
    private readonly surfaceToggle = document.createElement('sl-icon-button') as HTMLElement & { disabled: boolean };
    private surface: 'library' | 'playback' = 'library';
    private metadataKey = '';
    private metadataRequest = 0;
    private metadata?: OccurrenceDisplay;
    private readonly hints = new Map<HTMLElement, HTMLElement>();
    constructor(private readonly container: HTMLElement, private readonly onSurfaceChange: (surface: 'library' | 'playback') => void = () => {}) {
        container.className = 'playback-controls';
        container.setAttribute('aria-label', t('playback.controls'));
        const info = document.createElement('div');
        info.className = 'playback-controls__info';
        this.primary.setAttribute('variant', 'primary');
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
        timelineGroup.append(this.elapsed, this.timeline, this.duration);
        this.source.className = 'playback-controls__source';
        this.source.setAttribute('aria-hidden', 'true');
        this.sourceHint.className = 'playback-controls__source-hint';
        this.sourceHint.setAttribute('placement', 'top');
        this.sourceHint.setAttribute('hoist', '');
        this.sourceHint.append(this.source);
        this.artist.className = 'playback-controls__artist';
        const identity = document.createElement('div');
        identity.className = 'playback-controls__identity';
        identity.append(this.sourceHint, this.title);
        info.append(identity, this.artist);
        const outputGroup = document.createElement('div');
        outputGroup.className = 'playback-controls__output';
        this.outputDropdown.className = 'playback-controls__output-dropdown';
        this.outputDropdown.setAttribute('placement', 'top-end');
        this.outputDropdown.setAttribute('hoist', '');
        this.outputDropdown.addEventListener('keydown', event => {
            if (event.key !== 'Escape') return;
            event.preventDefault();
            event.stopPropagation();
            void (this.outputDropdown as HTMLElement & { hide(): Promise<void> }).hide().then(() => {
                if (!this.disposed) this.outputToggle.focus();
            });
        });
        const outputHint = document.createElement('sl-tooltip');
        outputHint.setAttribute('content', t('playback.output.label'));
        outputHint.setAttribute('hoist', '');
        outputHint.append(this.outputToggle);
        const outputTrigger = document.createElement('span');
        outputTrigger.className = 'playback-controls__output-trigger';
        outputTrigger.setAttribute('slot', 'trigger');
        outputTrigger.append(outputHint);
        this.outputToggle.setAttribute('name', 'speaker');
        this.outputToggle.setAttribute('label', t('playback.output.label'));
        const outputLabel = document.createElement('label');
        outputLabel.textContent = t('playback.output.label');
        this.outputSelect.setAttribute('aria-label', t('playback.output.label'));
        outputLabel.append(this.outputSelect);
        this.outputStatus.setAttribute('role', 'status');
        this.outputStatus.setAttribute('aria-live', 'polite');
        const refresh = document.createElement('button');
        this.setIcon(refresh, 'arrow-clockwise', t('playback.output.refresh'));
        refresh.addEventListener('click', () => void this.refreshOutputs());
        this.setIcon(this.outputReset, 'arrow-counterclockwise', t('playback.output.reset'));
        this.outputReset.addEventListener('click', () => {
            this.resetOutput = true;
            this.outputStatus.textContent = t('playback.output.reset_choose');
            this.outputSelect.focus();
        });
        this.outputReset.hidden = true;
        this.outputSelect.addEventListener('focus', () => void this.refreshOutputs());
        this.outputSelect.addEventListener('blur', () => this.renderOptions());
        this.outputSelect.addEventListener('change', () => void this.chooseOutput());
        outputGroup.append(outputLabel, this.hint(refresh), this.hint(this.outputReset), this.outputStatus);
        this.outputDropdown.append(outputTrigger, outputGroup);
        this.setSurface('library');
        this.setIcon(this.refresh, 'arrow-clockwise', t('playback.refresh'));
        this.refresh.addEventListener('click', () => void this.refreshPlayback());
        this.surfaceToggle.addEventListener('click', () => this.onSurfaceChange(this.surface === 'library' ? 'playback' : 'library'));
        const actions = document.createElement('div');
        actions.className = 'playback-controls__actions';
        actions.append(...[this.primary, this.returnToSession, this.stop, this.next, this.retry].map(button => this.hint(button)), this.outputDropdown, this.hint(this.surfaceToggle, 'top-end'), this.hint(this.refresh));
        this.messages.className = 'playback-controls__messages';
        this.messages.append(this.status, this.seekStatus, this.error);
        container.replaceChildren(info, actions, timelineGroup, this.messages);
        this.primary.hidden = this.returnToSession.hidden = this.stop.hidden = this.next.hidden = this.retry.hidden = true;
        window.addEventListener('pagehide', this.onPageHide, { once: true });
        void this.refreshServerLabels();
        this.unsubscribePlayback = playbackStore.subscribe(
            (snapshot, previous) => this.receiveSnapshot(snapshot, previous),
            () => {
                if (!this.container.isConnected) this.destroy();
                else void this.refreshOutputs(false);
            },
        );
        this.unsubscribeConnection = playbackStore.subscribeConnection(state => {
            if (this.disposed) return;
            if (state !== 'fresh') {
                this.invalidateInteractions();
            } else if (this.snapshot) {
                this.anchorPositionMs = this.snapshot.positionMs;
                this.anchorAt = this.now();
            }
            this.renderConnection();
        });
        this.scheduleRepaint();
        if (typeof ResizeObserver !== 'undefined') {
            this.resizeObserver = new ResizeObserver(() => {
                this.outputDropdown.style.setProperty('--playback-column-width', `${this.container.clientWidth}px`);
            });
            this.resizeObserver.observe(container);
        }
    }
    destroy(): void {
        this.disposed = true;
        this.resizeObserver?.disconnect();
        this.unsubscribeConnection?.();
        this.unsubscribePlayback?.();
        this.unsubscribePlayback = undefined;
        if (this.repaintTimer !== undefined) globalThis.clearInterval(this.repaintTimer);
        window.removeEventListener('pagehide', this.onPageHide);
    }
    setSurface(surface: 'library' | 'playback'): void {
        this.surface = surface;
        const showPlayback = surface === 'library';
        this.setIcon(this.surfaceToggle, showPlayback ? 'music-note-list' : 'collection',
            t(showPlayback ? 'playback.show_playing' : 'playback.browse_library'));
        this.surfaceToggle.setAttribute('aria-pressed', String(surface === 'playback'));
    }
    private receiveSnapshot(snapshot: PlaybackSessionSnapshot, previous?: PlaybackSessionSnapshot): void {
            if (!this.container.isConnected) { this.destroy(); return; }
            const current = this.snapshot;
            if (current && snapshot.instanceId === current.instanceId && snapshot.sessionId === current.sessionId
                && BigInt(snapshot.stateSequence) <= BigInt(current.stateSequence)) {
                void this.refreshOutputs(false);
                return;
            }
            if (!this.disposed && this.container.isConnected) {
                if (this.seekIdentity(previous) !== this.seekIdentity(snapshot)) {
                    this.invalidateInteractions();
                }
                this.snapshot = snapshot;
                this.anchorPositionMs = snapshot.positionMs;
                this.anchorAt = this.now();
                if (!this.scrubbing && !this.seekBusy && !snapshot.playback.pendingSeek
                    && snapshot.playback.seekOutcome?.status !== 'pending') this.scrubPreviewMs = undefined;
                this.loadMetadata(snapshot);
                this.render(snapshot);
                if (!previous || snapshot.instanceId !== previous.instanceId) {
                    this.resetOutput = false;
                    this.outputs = [];
                    void this.refreshOutputs();
                }
            }
    }
    private render(snapshot: PlaybackSessionSnapshot): void {
        const metadata = snapshot.playback.metadata ?? (snapshot.mode === 'main' ? this.metadata : undefined);
        this.setText(this.title, snapshot.current ? metadata?.title ?? t('playback.unknown_track') : t('playback.nothing_selected'));
        this.setText(this.artist, snapshot.current ? metadata?.artist ?? '' : '');
        const sourceId = snapshot.current?.source.serverId;
        const sourceIdentity = sourceId ? this.serverIdentities.get(sourceId) : undefined;
        const sourceLabel = sourceIdentity?.label;
        this.source.setAttribute('name', sourceIdentity?.icon ?? 'hdd-network');
        this.sourceHint.setAttribute('content', sourceLabel ?? t('playback.queue.source_unavailable'));
        this.sourceHint.hidden = !sourceId;
        const transportStatus = snapshot.output?.error ? t(`playback.error.${snapshot.output.error.code}`) : snapshot.playback.error
            ? t(`playback.error.${snapshot.playback.error.code}`)
            : t(`playback.status.${snapshot.playback.status}`);
        const sourceUnavailable = snapshot.mode === 'main' && (snapshot.mainCurrent?.availability === 'notConfigured' || this.metadata?.status === 'sourceUnavailable');
        const outputUnavailable = !snapshot.output?.selected?.available || Boolean(snapshot.output?.pending);
        const statusText = !snapshot.current ? t('playback.idle_guidance') : sourceUnavailable ? t('playback.queue.source_unavailable') : outputUnavailable && !snapshot.output?.error ? t('playback.output.choose') : snapshot.mode === 'preview'
            ? t('playback.status.preview', { status: transportStatus })
            : transportStatus;
        this.setText(this.status, this.fresh() ? statusText : t(`playback.connection.${playbackStore.connection()}`));
        this.setText(this.error, this.commandError);
        this.statusPopup = !this.fresh() || !snapshot.current || sourceUnavailable || outputUnavailable
            || snapshot.mode === 'preview' || Boolean(snapshot.output?.error || snapshot.playback.error)
            || ['loading', 'buffering'].includes(snapshot.playback.status);
        const action = snapshot.state === 'playing' || snapshot.state === 'buffering' ? 'pause' : 'resume';
        const label = t(action === 'resume' && (snapshot.output?.error?.code ?? snapshot.playback.error?.code) === 'OUTPUT_LOST'
            ? 'playback.resume_selected_output' : `playback.${action}`, { name: snapshot.output?.selected?.displayName ?? '' });
        this.primary.dataset.playbackAction = action;
        this.setIcon(this.primary, action === 'pause' ? 'pause-fill' : 'play-fill', label);
        this.primary.hidden = this.stop.hidden = !snapshot.current;
        this.primary.disabled = this.stop.disabled = this.busy || !this.fresh();
        this.next.hidden = !snapshot.current;
        this.next.disabled = this.busy || !this.fresh() || !snapshot.playback.canGoNext;
        this.retry.hidden = snapshot.playback.status !== 'error' || !snapshot.playback.error?.retryable;
        this.retry.disabled = this.busy || !this.fresh();
        this.returnToSession.hidden = snapshot.mode !== 'preview' || !snapshot.preview?.hasMainSession;
        this.returnToSession.disabled = this.busy || !this.fresh();
        if (action === 'resume' && snapshot.output && (!snapshot.output.selected?.available || snapshot.output.pending)) {
            this.primary.disabled = true;
            this.primary.setAttribute('title', t('playback.output.choose'));
        } else { this.primary.setAttribute('title', label); }
        if (action === 'resume' && sourceUnavailable) this.primary.disabled = true;
        this.outputSelect.disabled = this.outputBusy || !this.fresh();
        this.outputReset.hidden = snapshot.output?.error?.code !== 'OUTPUT_CONFIG_INVALID';
        const outputText = snapshot.output?.error
            ? t(`playback.error.${snapshot.output.error.code}`)
            : t(`playback.output.${snapshot.output?.status ?? 'unselected'}`);
        const active = snapshot.output?.active
            ? t('playback.output.active_name', { name: snapshot.output.active.displayName }) : '';
        if (!this.resetOutput) this.setText(this.outputStatus, `${outputText} ${active}`.trim());
        this.refresh.hidden = this.fresh() && (!snapshot.current || Boolean(sourceLabel && (snapshot.playback.metadata || this.metadata?.status === 'available')));
        for (const [button, hint] of this.hints) hint.hidden = button.hidden;
        this.renderOptions();
        this.renderTimeline();
    }

    private invalidateInteractions(): void {
        ++this.interactionEpoch;
        this.scrubbing = false;
        this.scrubPreviewMs = undefined;
        this.queuedSeek = undefined;
        this.busy = this.seekBusy = this.outputBusy = false;
    }

    private fresh(): boolean { return playbackStore.connection() === 'fresh'; }

    private renderConnection(): void {
        if (this.snapshot) this.render(this.snapshot);
        else {
            this.setText(this.status, t(`playback.connection.${playbackStore.connection()}`));
            this.statusPopup = true;
            this.outputSelect.disabled = true;
            this.renderTimeline();
        }
    }

    private async refreshPlayback(): Promise<void> {
        if (this.disposed) return;
        try {
            await playbackStore.refresh();
            if (!this.disposed && this.snapshot) {
                this.loadMetadata(this.snapshot, true);
                void this.refreshServerLabels();
                void this.refreshOutputs();
            }
        } catch { /* Shared connection status explains the failure. */ }

    }

    private setText(node: HTMLElement, value: string): void {
        if (node.textContent !== value) node.textContent = value;
    }

    private setIcon(button: HTMLElement, name: string, label: string): void {
        button.setAttribute('size', 'small');
        if (button.tagName.toLowerCase() === 'sl-icon-button') {
            button.setAttribute('name', name);
            button.setAttribute('label', label);
        }
        let icon = button.querySelector('sl-icon');
        if (!icon) { icon = document.createElement('sl-icon'); icon.setAttribute('aria-hidden', 'true'); button.append(icon); }
        icon.setAttribute('name', name);
        let accessibleLabel = button.querySelector('span');
        if (!accessibleLabel) {
            accessibleLabel = document.createElement('span');
            accessibleLabel.className = 'sr-only';
            button.append(accessibleLabel);
        }
        this.setText(accessibleLabel as HTMLElement, label);
        button.setAttribute('aria-label', label);
        this.hints?.get(button)?.setAttribute('content', label);
    }

    private hint(button: HTMLElement, placement = 'top'): HTMLElement {
        const hint = document.createElement('sl-tooltip');
        hint.setAttribute('content', button.getAttribute('aria-label') ?? '');
        hint.setAttribute('placement', placement);
        hint.setAttribute('hoist', '');
        hint.append(button);
        this.hints.set(button, hint);
        return hint;
    }

    private loadMetadata(snapshot: PlaybackSessionSnapshot, force = false): void {
        const key = JSON.stringify([snapshot.instanceId, snapshot.sessionId, snapshot.queueRevision, snapshot.generationId, snapshot.mode, snapshot.current, Boolean(snapshot.playback.metadata)]);
        if (key === this.metadataKey && !force) return;
        this.metadataKey = key;
        const request = ++this.metadataRequest;
        this.metadata = undefined;
        if (snapshot.mode !== 'main' || !snapshot.current || snapshot.playback.metadata) return;
        const occurrence = snapshot.current;
        void playbackDescribeOccurrences(snapshot, [occurrence.occurrenceId]).then(rows => {
            if (this.disposed || request !== this.metadataRequest || key !== this.metadataKey) return;
            this.metadata = rows.find(row => row.occurrenceId === occurrence.occurrenceId
                && row.source.serverId === occurrence.source.serverId && row.source.trackId === occurrence.source.trackId);
            if (this.snapshot) this.render(this.snapshot);
        }).catch(() => { /* Unknown title remains visible; explicit refresh can retry. */ });
    }

    private async refreshServerLabels(): Promise<void> {
        try {
            const servers = await serverList();
            if (this.disposed) return;
            this.serverIdentities = new Map(servers
                .filter(server => typeof server.serverId === 'string' && server.serverId.length > 0)
                .map(server => {
                    const identity = formatServerIdentity(server);
                    return [server.serverId as string, { label: identity.label, icon: identity.icon }];
                }));
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
        if (this.fresh() && snapshot.playback.status === 'active' && !snapshot.playback.pendingSeek && this.anchorAt > 0) {
            const age = Math.max(0, this.now() - this.anchorAt);
            position += Math.min(age, 750);
        }
        const duration = snapshot.playback.durationMs;
        return Math.max(0, duration && duration > 0 ? Math.min(position, duration) : position);
    }

    private renderTimeline(): void {
        const snapshot = this.snapshot;
        this.timeline.parentElement?.toggleAttribute('hidden', !snapshot?.current);
        const duration = snapshot?.playback.durationMs ?? 0;
        const available = this.fresh() && Boolean(snapshot?.current && snapshot.playback.seek?.available && duration > 0);
        const shown = Math.round(this.shownPositionMs());
        this.timeline.max = String(duration > 0 ? duration : 1);
        this.timeline.value = String(Math.min(this.scrubPreviewMs ?? shown, duration > 0 ? duration : 0));
        this.timeline.disabled = !available;
        this.timeline.setAttribute('aria-valuetext', t('playback.seek.value', {
            elapsed: this.formatTime(this.scrubPreviewMs ?? shown), duration: duration > 0 ? this.formatTime(duration) : t('playback.seek.unknown_duration'),
        }));
        this.setText(this.elapsed, this.formatTime(shown));
        this.setText(this.duration, duration > 0 ? this.formatTime(duration) : t('playback.seek.unknown_duration'));
        if (snapshot?.playback.pendingSeek) {
            this.setText(this.seekStatus, t('playback.seek.pending'));
        } else if (snapshot?.playback.seekOutcome?.status === 'failed') {
            this.setText(this.seekStatus, t('playback.seek.failed'));
        } else if (!available) {
            this.setText(this.seekStatus, snapshot?.current && this.fresh() ? t(snapshot.playback.seek?.reason ?? 'playback.seek.unavailable') : '');
        } else {
            this.setText(this.seekStatus, '');
        }
        this.updateMessages();
    }

    private updateMessages(): void {
        const visible = this.statusPopup || Boolean(this.seekStatus.textContent || this.error.textContent);
        this.messages.className = `playback-controls__messages${visible ? ' is-visible' : ''}`;
    }

    private formatTime(valueMs: number): string {
        const seconds = Math.max(0, Math.floor(valueMs / 1000));
        return `${Math.floor(seconds / 60)}:${String(seconds % 60).padStart(2, '0')}`;
    }

    private now(): number {
        return globalThis.performance?.now?.() ?? Date.now();
    }

    private seekIdentity(snapshot: PlaybackSessionSnapshot | undefined): string {
        return JSON.stringify([snapshot?.instanceId, snapshot?.sessionId, snapshot?.generationId, snapshot?.current?.occurrenceId]);
    }

    private async submitSeek(positionMs: number): Promise<void> {
        if (this.disposed || !this.fresh() || !this.snapshot || !this.snapshot.playback.seek.available) return;
        const identity = this.seekIdentity(this.snapshot);
        const epoch = this.interactionEpoch;
        if (this.seekBusy) { this.queuedSeek = { positionMs, identity }; return; }
        this.seekBusy = true;
        this.commandError = '';
        this.render(this.snapshot);
        const observed = this.snapshot;
        try {
            const current = await playbackSeek(positionMs, observed);
            if (!this.disposed && epoch === this.interactionEpoch && this.seekIdentity(this.snapshot) === identity) {
                playbackStore.acceptCommand(current, observed);
            }
        } catch (error) {
            if (!this.disposed && epoch === this.interactionEpoch && this.seekIdentity(this.snapshot) === identity) {
                const code = (error as { data?: { code?: string } })?.data?.code;
                this.commandError = code ? t(`playback.error.${code}`) : t('playback.command_error');
                this.scrubPreviewMs = undefined;
                if (code?.includes('CONFLICT') || code?.includes('MISMATCH')) void playbackStore.refresh().catch(() => {});
            }
        } finally {
            if (this.disposed || epoch !== this.interactionEpoch) return;
            this.seekBusy = false;
            const queued = this.queuedSeek;
            this.queuedSeek = undefined;
            if (!this.disposed && this.fresh() && epoch === this.interactionEpoch && queued && queued.identity === this.seekIdentity(this.snapshot)
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
        if (this.disposed || !this.fresh() || this.outputBusy || !this.snapshot || !this.outputSelect.value) return;
        const observed = this.snapshot;
        const epoch = this.interactionEpoch;
        const identity = this.seekIdentity(observed);
        const outputId = this.outputSelect.value;
        this.outputBusy = true;
        this.commandError = '';
        this.render(observed);
        try {
            const current = await playbackSelectOutput(outputId, observed, this.resetOutput);
            if (!this.disposed && epoch === this.interactionEpoch && this.seekIdentity(this.snapshot) === identity) {
                playbackStore.acceptCommand(current, observed);
                this.resetOutput = false;
            }
        } catch (error) {
            if (!this.disposed && epoch === this.interactionEpoch && this.seekIdentity(this.snapshot) === identity) {
                const code = (error as { data?: { code?: string } })?.data?.code;
                this.commandError = code?.startsWith('OUTPUT_') ? t(`playback.error.${code}`) : t('playback.command_error');
                try { await playbackStore.refresh(); } catch { /* Preserve the command error. */ }
            }
        } finally {
            if (this.disposed || epoch !== this.interactionEpoch) return;
            this.outputBusy = false;
            if (!this.disposed && this.snapshot) this.render(this.snapshot);
        }
    }
    private button(action: 'pause' | 'resume' | 'stop' | 'next' | 'retry' | 'returnToSession'): HTMLElement & { disabled: boolean } {
        const button = document.createElement('sl-button') as any;
        button.size = 'small';
        button.dataset.playbackAction = action;
        const labelKey = action === 'returnToSession' ? 'playback.return_to_session' : `playback.${action}`;
        this.setIcon(button, { pause: 'pause-fill', resume: 'play-fill', stop: 'stop-fill', next: 'skip-end-fill', retry: 'arrow-clockwise', returnToSession: 'arrow-return-left' }[action], t(labelKey));
        button.addEventListener('click', async () => {
            if (button.disabled || this.busy || this.disposed || !this.fresh() || !this.snapshot) return;
            this.busy = true;
            this.commandError = '';
            const selected = this.snapshot;
            const identity = this.seekIdentity(selected);
            const epoch = this.interactionEpoch;
            this.render(selected);
            try {
                await playbackControl(button.dataset.playbackAction, selected);
                if (!this.disposed && epoch === this.interactionEpoch) await playbackStore.refresh();
            } catch (error) {
                if (this.disposed || epoch !== this.interactionEpoch || identity !== this.seekIdentity(this.snapshot)) return;
                const code = (error as { data?: { code?: string } })?.data?.code;
                if (code?.includes('CONFLICT') || code?.includes('MISMATCH')) void playbackStore.refresh().catch(() => {});
                this.commandError = t(code === 'PERSISTENCE_FAILED'
                    ? 'playback.command_error.persistence' : 'playback.command_error');
            } finally {
                if (this.disposed || epoch !== this.interactionEpoch) return;
                this.busy = false;
                if (!this.disposed && this.snapshot) this.render(this.snapshot);
            }
        });
        return button;
    }
}
