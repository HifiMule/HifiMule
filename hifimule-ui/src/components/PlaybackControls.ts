import { playbackControl, playbackGetSession, playbackListOutputs, playbackSelectOutput, PlaybackOutput, PlaybackSessionSnapshot } from '../rpc';
import { t } from '../i18n';

export class PlaybackControls {
    private disposed = false;
    private timer: ReturnType<typeof setTimeout> | undefined;
    private snapshot: PlaybackSessionSnapshot | undefined;
    private busy = false;
    private outputBusy = false;
    private discovering = false;
    private lastDiscoveryAt = 0;
    private resetOutput = false;
    private outputs: PlaybackOutput[] = [];
    private optionsSignature = '';
    private readonly outputSelect = document.createElement('select');
    private readonly outputStatus = document.createElement('span');
    private readonly outputReset = document.createElement('button');
    private readonly outputDropdown = document.createElement('sl-dropdown');
    private readonly outputToggle = document.createElement('sl-icon-button');
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
        container.replaceChildren(info, this.primary, this.stop, this.outputDropdown);
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
                if (!previous || snapshot.instanceId !== previous.instanceId) {
                    this.resetOutput = false;
                    this.outputs = [];
                    void this.refreshOutputs();
                }
            }
        } catch { /* daemon lifecycle UI owns connection errors */ }
        if (!this.disposed) {
            // listOutputs returns the owned worker's cached inventory. Pick up
            // its completed refresh through the existing polling lifecycle.
            if (this.snapshot) void this.refreshOutputs(false);
            this.timer = globalThis.setTimeout(() => void this.poll(), 500);
        }
    }
    private render(snapshot: PlaybackSessionSnapshot): void {
        this.title.textContent = snapshot.playback.metadata?.title ?? t('playback.nothing_selected');
        const statusText = snapshot.playback.error
            ? t(`playback.error.${snapshot.playback.error.code}`)
            : t(`playback.status.${snapshot.playback.status}`);
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
