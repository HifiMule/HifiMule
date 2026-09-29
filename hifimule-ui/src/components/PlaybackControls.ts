import { playbackControl, playbackDescribeOccurrences, playbackStartSelection, OccurrenceDisplay, playbackListOutputs, playbackListLiveReports, type LiveReport, playbackSeek, playbackSelectOutput, serverList, PlaybackOutput, PlaybackSessionSnapshot } from '../rpc';
import { t } from '../i18n';
import { formatServerIdentity } from '../serverIdentity';
import { playbackStore } from '../state/playback';
import { playbackGetFeedback, playbackSetFeedback, type PlaybackFeedback, type PlaybackFeedbackOperation, type PlaybackPreference } from '../rpc';

export class PlaybackControls {
    private disposed = false;
    private unsubscribePlayback?: () => void;
    private unsubscribeConnection?: () => void;
    private interactionEpoch = 0;
    private resizeObserver?: ResizeObserver;
    private repaintTimer: ReturnType<typeof setInterval> | undefined;
    private reportTimer: ReturnType<typeof setInterval> | undefined;
    private reportRequest = 0;
    private reports: LiveReport[] = [];
    private readonly reportStatus = document.createElement('div');
    private readonly feedbackLike = document.createElement('button');
    private readonly feedbackDislike = document.createElement('button');
    private readonly feedbackClear = document.createElement('button');
    private readonly feedbackStatus = document.createElement('span');
    private feedback?: PlaybackFeedback;
    private feedbackEpoch = 0;
    private feedbackRequest = 0;
    private feedbackReading = false;
    private feedbackLoading = false;
    private feedbackQueued = 0;
    private feedbackTotalQueued = 0;
    private feedbackAction = 0;
    private feedbackSerial: Promise<void> = Promise.resolve();
    private feedbackIssue = '';
    private feedbackLocalFailure?: PlaybackFeedbackOperation;
    private snapshot: PlaybackSessionSnapshot | undefined;
    private busy = false;
    private startBusy = false;
    private startRequest = 0;
    private startIssue: 'settings' | 'output' | null = null;
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
    private readonly startStatus = document.createElement('span');
    private readonly source = document.createElement('sl-icon');
    private readonly sourceLabel = document.createElement('span');
    private readonly sourceHint = document.createElement('sl-tooltip');
    private readonly error = document.createElement('span');
    private readonly messages = document.createElement('div');
    private statusPopup = true;
    private messagesOverride?: boolean;
    private messageKey = '';
    private messagesVisible = false;
    private readonly messagesToggle = document.createElement('sl-icon-button');
    private readonly back = this.button('back');
    private readonly primary = this.button('resume');
    private readonly playSomething = document.createElement('sl-button') as HTMLElement & { disabled: boolean };
    private readonly startRoute = document.createElement('button');
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
    constructor(private readonly container: HTMLElement, private readonly onSurfaceChange: (surface: 'library' | 'playback' | 'settings') => void = () => {}) {
        container.className = 'playback-controls';
        container.setAttribute('aria-label', t('playback.controls'));
        const info = document.createElement('div');
        info.className = 'playback-controls__info';
        this.primary.setAttribute('variant', 'primary');
        this.status.className = 'playback-controls__status';
        this.status.setAttribute('role', 'status');
        this.status.setAttribute('aria-live', 'polite');
        this.status.setAttribute('aria-atomic', 'true');
        this.startStatus.setAttribute('role', 'status');
        this.startStatus.setAttribute('aria-live', 'polite');
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
        this.sourceHint.hidden = true;
        this.sourceHint.setAttribute('placement', 'top');
        this.sourceHint.setAttribute('hoist', '');
        this.sourceLabel.tabIndex = 0;
        this.sourceLabel.className = 'playback-controls__source-label';
        this.sourceLabel.setAttribute('role', 'img');
        this.sourceLabel.append(this.source);
        this.sourceHint.append(this.sourceLabel);
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
        this.setIcon(this.messagesToggle, 'info-circle', t('playback.guidance.hide'));
        this.playSomething.textContent = t('playback.play_something');
        this.playSomething.className = 'playback-controls__play-something';
        this.playSomething.setAttribute('aria-description', t('playback.play_something_help'));
        this.playSomething.disabled = true;
        this.startRoute.className = 'playback-controls__start-route';
        this.startRoute.hidden = true;
        this.playSomething.addEventListener('click', () => void this.startRadio());
        this.startRoute.type = 'button';
        this.startRoute.addEventListener('click', () => {
            if (this.startIssue === 'settings') this.onSurfaceChange('settings');
            else if (this.startIssue === 'output') this.openOutputChoice();
        });
        this.messages.id = 'playback-guidance';
        this.messagesToggle.setAttribute('aria-controls', this.messages.id);
        this.messagesToggle.addEventListener('click', () => {
            this.messagesOverride = !this.messagesVisible;
            this.updateMessages();
        });
        this.surfaceToggle.addEventListener('click', () => this.onSurfaceChange(this.surface === 'library' ? 'playback' : 'library'));
        const actions = document.createElement('div');
        actions.className = 'playback-controls__actions';
        for (const [button, value, icon] of [
            [this.feedbackLike, 'like', 'hand-thumbs-up'],
            [this.feedbackDislike, 'dislike', 'hand-thumbs-down'],
            [this.feedbackClear, 'neutral', 'x-circle'],
        ] as const) {
            button.type = 'button';
            button.hidden = true;
            button.dataset.playbackFeedback = value;
            this.setIcon(button, icon, t(`playback.feedback.action_${value}`));
            button.addEventListener('click', () => this.submitFeedback(value));
        }
        actions.append(...[this.back, this.primary, this.returnToSession, this.stop, this.next, this.retry, this.feedbackLike, this.feedbackDislike, this.feedbackClear].map(button => this.hint(button)), this.playSomething, this.startRoute, this.outputDropdown, this.hint(this.surfaceToggle, 'top-end'), this.hint(this.messagesToggle, 'top-end', 16), this.hint(this.refresh, 'top-end', 16));
        this.messages.className = 'playback-controls__messages';
        this.reportStatus.setAttribute('role', 'status');
        this.reportStatus.setAttribute('aria-live', 'polite');
        this.feedbackStatus.setAttribute('role', 'status');
        this.feedbackStatus.setAttribute('aria-live', 'polite');
        this.feedbackStatus.setAttribute('aria-atomic', 'true');
        this.messages.append(this.status, this.startStatus, this.seekStatus, this.error, this.reportStatus, this.feedbackStatus);
        container.replaceChildren(info, actions, timelineGroup, this.messages);
        this.back.hidden = this.primary.hidden = this.returnToSession.hidden = this.stop.hidden = this.next.hidden = this.retry.hidden = true;
        window.addEventListener('pagehide', this.onPageHide, { once: true });
        void this.refreshServerLabels();
        this.unsubscribePlayback = playbackStore.subscribe(
            (snapshot, previous) => this.receiveSnapshot(snapshot, previous),
            () => {
                if (!this.container.isConnected) this.destroy();
                else void this.refreshOutputs(false);
            },
        );
        if (typeof globalThis.setInterval === 'function') {
            this.reportTimer = globalThis.setInterval(() => {
                void this.refreshLiveReports();
                void this.refreshFeedback();
            }, 5000);
        }
        this.unsubscribeConnection = playbackStore.subscribeConnection(state => {
            if (this.disposed) return;
            if (state !== 'fresh') {
                this.invalidateInteractions();
                this.resetFeedback(true);
            } else if (this.snapshot) {
                this.anchorPositionMs = this.snapshot.positionMs;
                this.anchorAt = this.now();
                void this.refreshFeedback();
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
        this.resetFeedback();
        ++this.startRequest;
        this.resizeObserver?.disconnect();
        this.unsubscribeConnection?.();
        this.unsubscribePlayback?.();
        this.unsubscribePlayback = undefined;
        if (this.repaintTimer !== undefined) globalThis.clearInterval(this.repaintTimer);
        if (this.reportTimer !== undefined) globalThis.clearInterval(this.reportTimer);
        window.removeEventListener('pagehide', this.onPageHide);
    }
    openOutputChoice(): void {
        if (this.disposed) return;
        this.outputToggle.focus();
        this.outputToggle.click();
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
                    // Our own successful start changes the session identity before
                    // the RPC reply can arrive. Keep its progress tied to that reply.
                    this.invalidateInteractions(true, this.startBusy && snapshot.queueKind === 'radio');
                }
                this.snapshot = snapshot;
                if (this.feedbackIdentity(current) !== this.feedbackIdentity(snapshot)) {
                    this.resetFeedback();
                    void this.refreshFeedback();
                }
                this.anchorPositionMs = snapshot.positionMs;
                this.anchorAt = this.now();
                if (!this.scrubbing && !this.seekBusy && !snapshot.playback.pendingSeek
                    && snapshot.playback.seekOutcome?.status !== 'pending') this.scrubPreviewMs = undefined;
                this.loadMetadata(snapshot);
                if (!previous || snapshot.sessionId !== previous.sessionId || snapshot.current?.occurrenceId !== previous.current?.occurrenceId) {
                    this.reports = [];
                    void this.refreshLiveReports();
                }
                this.render(snapshot);
                if (!previous || snapshot.instanceId !== previous.instanceId) {
                    this.resetOutput = false;
                    this.outputs = [];
                    void this.refreshOutputs();
                }
            }
    }
    private render(snapshot: PlaybackSessionSnapshot): void {
        this.renderFeedback();
        const currentReports = this.reports.filter(report => snapshot.current
            && report.occurrenceId === snapshot.current.occurrenceId
            && report.serverId === snapshot.current.source.serverId);
        this.reportStatus.replaceChildren(...currentReports.slice(0, 3).map(report => {
            const line = document.createElement('div');
            const source = this.serverIdentities.get(report.serverId)?.label ?? t('playback.reporting.source');
            line.textContent = `${source} (${report.occurrenceId.slice(0, 8)}): ${t(`playback.reporting.${report.status}`)}`;
            return line;
        }));
        const metadata = snapshot.playback.metadata ?? (snapshot.mode === 'main' ? this.metadata : undefined);
        this.setText(this.title, snapshot.current ? metadata?.title ?? t('playback.unknown_track') : t('playback.nothing_selected'));
        this.setText(this.artist, snapshot.current ? metadata?.artist ?? '' : '');
        const sourceId = snapshot.current?.source.serverId;
        const sourceIdentity = sourceId ? this.serverIdentities.get(sourceId) : undefined;
        const sourceLabel = sourceIdentity?.label;
        this.source.setAttribute('name', sourceIdentity?.icon ?? 'hdd-network');
        this.sourceHint.setAttribute('content', sourceLabel ?? t('playback.queue.source_unavailable'));
        this.sourceLabel.setAttribute('aria-label', t('playback.source', { source: sourceLabel ?? t('playback.queue.source_unavailable') }));
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
        const continuityNotice = snapshot.mode === 'main' && snapshot.current && snapshot.continuityStatus
            ? t(`playback.book_progress.${snapshot.continuityStatus}`) : '';
        this.setText(this.error, [this.commandError, continuityNotice].filter(Boolean).join(' '));
        this.statusPopup = !this.fresh() || !snapshot.current || sourceUnavailable || outputUnavailable || Boolean(this.startStatus.textContent)
            || snapshot.mode === 'preview' || Boolean(snapshot.output?.error || snapshot.playback.error)
            || ['loading', 'buffering'].includes(snapshot.playback.status);
        const action = snapshot.state === 'playing' || snapshot.state === 'buffering' ? 'pause' : 'resume';
        const label = t(action === 'resume' && (snapshot.output?.error?.code ?? snapshot.playback.error?.code) === 'OUTPUT_LOST'
            ? 'playback.resume_selected_output' : `playback.${action}`, { name: snapshot.output?.selected?.displayName ?? '' });
        this.primary.dataset.playbackAction = action;
        this.setIcon(this.primary, action === 'pause' ? 'pause-fill' : 'play-fill', label);
        this.primary.hidden = this.stop.hidden = !snapshot.current;
        this.playSomething.hidden = Boolean(snapshot.current && snapshot.state !== 'paused' && snapshot.state !== 'stopped');
        this.playSomething.disabled = this.startBusy || !this.fresh();
        this.startRoute.hidden = this.startIssue === null;
        this.startRoute.textContent = t(this.startIssue === 'output' ? 'playback.output.choose' : 'playback.selection.open_settings');
        this.primary.disabled = this.stop.disabled = this.busy || !this.fresh();
        this.back.hidden = !snapshot.current;
        this.back.disabled = this.busy || !this.fresh() || !snapshot.playback.canGoBack;
        const backHelp = snapshot.playback.canGoBack
            ? t('playback.back_help')
            : t(snapshot.playback.backUnavailableReason ?? 'playback.back_unavailable');
        this.back.dataset.playbackHelp = backHelp;
        this.back.setAttribute('title', backHelp);
        this.back.setAttribute('aria-description', backHelp);
        this.hints.get(this.back)?.setAttribute('content', backHelp);
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
        this.renderRefresh();
        for (const [button, hint] of this.hints) hint.hidden = button.hidden;
        this.renderOptions();
        this.renderTimeline();
    }

    private async refreshLiveReports(): Promise<void> {
        const sessionId = this.snapshot?.sessionId;
        if (!sessionId || this.disposed) return;
        const request = ++this.reportRequest;
        try {
            const reports = await playbackListLiveReports(sessionId);
            if (this.disposed || request !== this.reportRequest || this.snapshot?.sessionId !== sessionId) return;
            this.reports = reports;
            if (this.snapshot) this.render(this.snapshot);
        } catch { /* Playback remains usable; the next refresh can recover status. */ }
    }

    private invalidateInteractions(clearCommandError = false, preserveStart = false): void {
        ++this.interactionEpoch;
        if (!preserveStart) {
            ++this.startRequest;
            this.startBusy = false;
            this.startIssue = null;
            this.startStatus.textContent = '';
        }
        this.scrubbing = false;
        this.scrubPreviewMs = undefined;
        this.queuedSeek = undefined;
        this.busy = this.seekBusy = this.outputBusy = false;
        if (clearCommandError) this.commandError = '';
    }

    private fresh(): boolean { return playbackStore.connection() === 'fresh'; }

    private feedbackIdentity(snapshot = this.snapshot): string {
        return JSON.stringify([snapshot?.instanceId, snapshot?.sessionId, snapshot?.radio?.logicalId,
            snapshot?.current?.occurrenceId, snapshot?.current?.source.serverId, snapshot?.current?.source.trackId]);
    }

    private resetFeedback(preserveControls = false): void {
        ++this.feedbackEpoch;
        ++this.feedbackRequest;
        this.feedback = preserveControls && this.feedback ? { ...this.feedback, preference: null, readStatus: 'unknown' } : undefined;
        this.feedbackReading = this.feedbackLoading = false;
        this.feedbackQueued = 0;
        this.feedbackIssue = '';
        this.feedbackLocalFailure = undefined;
    }

    private feedbackMatches(target: PlaybackFeedback['target'], observed: PlaybackSessionSnapshot): boolean {
        return target.sessionId === observed.sessionId && target.logicalSessionId === (observed.radio?.logicalId ?? observed.sessionId)
            && target.occurrenceId === observed.current?.occurrenceId && target.source.serverId === observed.current?.source.serverId
            && target.source.trackId === observed.current?.source.trackId;
    }

    private async refreshFeedback(): Promise<void> {
        if (this.disposed || !this.fresh() || !this.snapshot?.current || this.feedbackReading || this.feedbackQueued) return;
        const observed = this.snapshot;
        const identity = this.feedbackIdentity(observed);
        const request = ++this.feedbackRequest;
        this.feedbackReading = true;
        this.feedbackLoading = !this.feedback;
        this.renderFeedback();
        try {
            const result = await playbackGetFeedback(observed);
            if (this.disposed || request !== this.feedbackRequest || identity !== this.feedbackIdentity() || !this.feedbackMatches(result.target, observed)) return;
            this.feedback = result.readStatus === 'unknown' && this.feedback
                ? { ...result, capabilities: this.feedback.capabilities } : result;
            if (this.feedbackLocalFailure) this.feedback.operation = this.feedbackLocalFailure;
            this.feedbackIssue = '';
        } catch {
            if (this.disposed || request !== this.feedbackRequest || identity !== this.feedbackIdentity()) return;
            // A failed read cannot turn a cached value into current server evidence.
            if (this.feedback) this.feedback = { ...this.feedback, preference: null, readStatus: 'unknown' };
            this.feedbackIssue = 'unknown';
        } finally {
            if (!this.disposed && request === this.feedbackRequest) {
                this.feedbackReading = this.feedbackLoading = false;
                this.renderFeedback();
            }
        }
    }

    private submitFeedback(value: PlaybackPreference): void {
        const capability = value === 'neutral' ? 'clear' : value;
        if (this.disposed || !this.fresh() || !this.snapshot?.current || !this.feedback?.capabilities[capability] || this.feedbackTotalQueued >= 16) return;
        const observed = this.snapshot;
        const identity = this.feedbackIdentity(observed);
        const epoch = this.feedbackEpoch;
        const action = ++this.feedbackAction;
        ++this.feedbackRequest;
        this.feedbackReading = this.feedbackLoading = false;
        this.feedbackLocalFailure = undefined;
        this.feedbackIssue = '';
        ++this.feedbackQueued;
        ++this.feedbackTotalQueued;
        this.renderFeedback();
        // Admission is ordered too: a slower first preflight must not reverse two clicks.
        this.feedbackSerial = this.feedbackSerial.then(async () => {
            if (this.disposed || epoch !== this.feedbackEpoch || identity !== this.feedbackIdentity() || !this.fresh()) return;
            try {
                const operation = await playbackSetFeedback(observed, value);
                if (this.disposed || epoch !== this.feedbackEpoch || action !== this.feedbackAction || !this.feedbackMatches(operation.target, observed)) return;
                if (this.feedback) {
                    this.feedback.operation = operation;
                    if (value !== 'neutral') this.feedback.rejected = value === 'dislike';
                }
                if (operation.sequence === '0') this.feedbackLocalFailure = operation;
            } catch (error) {
                if (this.disposed || epoch !== this.feedbackEpoch || action !== this.feedbackAction) return;
                // A lost RPC reply may hide a durable admission. Refresh; never replay it.
                this.feedbackIssue = (error as { data?: { code?: string } })?.data?.code ? 'failed' : 'ambiguous';
            }
        }).finally(() => {
            --this.feedbackTotalQueued;
            if (!this.disposed && epoch === this.feedbackEpoch) {
                --this.feedbackQueued;
                this.renderFeedback();
            }
        });
    }

    private renderFeedback(): void {
        const current = Boolean(this.snapshot?.current);
        for (const [button, capability, value] of [
            [this.feedbackLike, 'like', 'like'], [this.feedbackDislike, 'dislike', 'dislike'], [this.feedbackClear, 'clear', 'neutral'],
        ] as const) {
            button.hidden = !current || !this.feedback?.capabilities[capability];
            button.disabled = !this.fresh() || this.feedback?.readStatus !== 'known' || this.feedbackTotalQueued >= 16 || !this.feedback?.capabilities[capability];
            const hint = this.hints.get(button);
            if (hint) hint.hidden = button.hidden;
            if (value !== 'neutral') button.setAttribute('aria-pressed', String(this.fresh() && this.feedback?.readStatus === 'known' && this.feedback.preference === value));
        }
        let label = '';
        if (current) {
            const operation = this.feedback?.operation;
            const state = !this.fresh() ? 'unknown' : this.feedbackQueued ? 'pending' : this.feedbackIssue || (this.feedbackLoading ? 'loading' : '');
            const preference = this.feedback?.readStatus === 'known' ? this.feedback.preference : this.feedback?.readStatus ?? 'unknown';
            label = [t(`playback.feedback.${state || preference}`),
                !state && operation ? t(`playback.feedback.${operation.status}`) : '',
                this.feedback?.rejected ? t('playback.feedback.rejected') : '',
            ].filter(Boolean).join(' ');
        }
        this.setText(this.feedbackStatus, label);
        this.renderRefresh();
        this.updateMessages();
    }

    private renderRefresh(): void {
        const snapshot = this.snapshot;
        const feedbackNeedsRefresh = snapshot?.current && (this.feedbackIssue || this.feedback?.readStatus === 'unknown'
            || ['failed', 'ambiguous', 'conflict'].includes(this.feedback?.operation?.status ?? ''));
        const sourceLabel = snapshot?.current && this.serverIdentities.get(snapshot.current.source.serverId)?.label;
        this.refresh.hidden = this.fresh() && !feedbackNeedsRefresh
            && (!snapshot?.current || Boolean(sourceLabel && (snapshot.playback.metadata || this.metadata?.status === 'available')));
    }

    private async startRadio(): Promise<void> {
        if (this.disposed || !this.fresh() || this.startBusy) return;
        const request = ++this.startRequest;
        this.startBusy = true;
        this.startIssue = null;
        this.startStatus.textContent = t('playback.selection.starting');
        if (this.snapshot) this.render(this.snapshot);
        try {
            await playbackStartSelection();
            if (this.disposed || request !== this.startRequest) return;
            this.startStatus.textContent = t('playback.selection.started');
            try { await playbackStore.refresh(); }
            catch { /* Admission succeeded; the shared connection state reports read failure. */ }
        } catch (error) {
            if (this.disposed || request !== this.startRequest) return;
            const code = (error as { data?: { code?: string } })?.data?.code ?? String(error);
            if (code.includes('OUTPUT_')) {
                this.startIssue = 'output';
                this.startStatus.textContent = t('playback.output.choose');
            } else if (code.includes('PLAYBACK_SELECTION_CANCELLED')) {
                this.startStatus.textContent = t('playback.selection.cancelled');
            } else if (code.includes('PLAYBACK_BUSY')) {
                this.startStatus.textContent = t('playback.selection.busy');
            } else if (code.includes('PERSISTENCE_FAILED')) {
                this.startStatus.textContent = t('playback.command_error.persistence');
            } else {
                this.startIssue = 'settings';
                this.startStatus.textContent = t(code.includes('PLAYBACK_SELECTION_SAVE_FAILED') ? 'playback.selection.load_failed'
                    : code.includes('PLAYBACK_SELECTION_PREPARATION_FAILED') ? 'playback.selection.preparation_failed'
                    : code.includes('PLAYBACK_SELECTION_EMPTY') ? 'playback.selection.empty'
                    : code.includes('PLAYBACK_SELECTION_SETUP') ? 'playback.selection.invalid' : 'playback.selection.unavailable');
            }
        } finally {
            if (!this.disposed && request === this.startRequest) {
                this.startBusy = false;
                if (this.snapshot) this.render(this.snapshot);
            }
        }
    }

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
                void this.refreshFeedback();
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
        this.hints?.get(button)?.setAttribute('content', button.dataset.playbackHelp ?? label);
    }

    private hint(button: HTMLElement, placement = 'top', distance?: number): HTMLElement {
        const hint = document.createElement('sl-tooltip');
        hint.setAttribute('content', button.dataset.playbackHelp ?? button.getAttribute('aria-label') ?? '');
        hint.setAttribute('placement', placement);
        if (distance !== undefined) hint.setAttribute('distance', String(distance));
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
        const key = JSON.stringify([this.statusPopup, this.status.textContent, this.startStatus.textContent, this.seekStatus.textContent, this.error.textContent, this.feedbackStatus.textContent]);
        if (key !== this.messageKey) {
            this.messageKey = key;
            this.messagesOverride = undefined;
        }
        const feedbackAttention = this.feedbackLoading || this.feedbackQueued > 0 || Boolean(this.feedbackIssue || this.feedback?.operation);
        this.messagesVisible = this.messagesOverride ?? (this.statusPopup || feedbackAttention || Boolean(this.startStatus.textContent || this.seekStatus.textContent || this.error.textContent));
        this.messages.className = `playback-controls__messages${this.messagesVisible ? ' is-visible' : ''}`;
        this.messagesToggle.setAttribute('aria-expanded', String(this.messagesVisible));
        this.setIcon(this.messagesToggle, 'info-circle', t(this.messagesVisible ? 'playback.guidance.hide' : 'playback.guidance.show'));
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
        let discardQueued = false;
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
                if (code?.includes('CONFLICT') || code?.includes('MISMATCH')) {
                    discardQueued = true;
                    this.queuedSeek = undefined;
                    try { await playbackStore.refresh(); } catch { /* Preserve the scoped command error. */ }
                }
            }
        } finally {
            if (this.disposed || epoch !== this.interactionEpoch) return;
            this.seekBusy = false;
            if (discardQueued) this.queuedSeek = undefined;
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
    private button(action: 'back' | 'pause' | 'resume' | 'stop' | 'next' | 'retry' | 'returnToSession'): HTMLElement & { disabled: boolean } {
        const button = document.createElement('sl-button') as any;
        button.size = 'small';
        button.dataset.playbackAction = action;
        const labelKey = action === 'returnToSession' ? 'playback.return_to_session' : `playback.${action}`;
        if (action === 'back') button.dataset.playbackHelp = t('playback.back_help');
        this.setIcon(button, { back: 'skip-start-fill', pause: 'pause-fill', resume: 'play-fill', stop: 'stop-fill', next: 'skip-end-fill', retry: 'arrow-clockwise', returnToSession: 'arrow-return-left' }[action], t(labelKey));
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
                    ? 'playback.command_error.persistence'
                    : code === 'BACK_UNAVAILABLE'
                        ? this.snapshot?.playback.backUnavailableReason ?? 'playback.error.BACK_UNAVAILABLE'
                        : 'playback.command_error');
            } finally {
                if (this.disposed || epoch !== this.interactionEpoch) return;
                this.busy = false;
                if (!this.disposed && this.snapshot) this.render(this.snapshot);
            }
        });
        return button;
    }
}
