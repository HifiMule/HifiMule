import { rpcCall } from '../rpc';
import { t } from '../i18n';
import { escapeAttr } from '../html';
import { type DeviceProfile, type DeviceProfileSummary, type ProfileDraft, formats, outputs, limits, toDraft, fromDraft, newProfile, validateDraft, unsupportedRules, profileOptions, displayProfile, profileNumericErrors, profileNumericRules, type ProfileConstraints } from '../deviceProfiles';

import { fieldFeedback, revealInvalid } from '../numericValidation';

type OpenOptions = { selectedId?: string; create?: boolean; onSaved?: (profile: DeviceProfile) => void | Promise<void>; returnFocus?: HTMLElement };
let instance: DeviceProfileManager | undefined;
export function openDeviceProfiles(options: OpenOptions = {}): Promise<void> {
    if (instance) { instance.focus(); return Promise.resolve(); }
    instance = new DeviceProfileManager(options); return instance.open();
}
export class DeviceProfileManager {
    private dialog: any;
    private profiles: DeviceProfileSummary[] = [];
    private profile?: DeviceProfile;
    private draft?: ProfileDraft;
    private constraints?: ProfileConstraints;
    private numericTouched = new Set<string>();
    private serverErrors: Record<string, string> = {};
    private clean = '';
    private creating = false;
    private busy = false;
    private closing = false;
    private cloneSource?: string;
    constructor(private options: OpenOptions = {}) {}
    focus(): void { this.dialog?.querySelector('#profile-name')?.focus(); }
    async open(): Promise<void> {
        this.dialog = document.createElement('sl-dialog');
        this.dialog.label = t('profiles.title'); this.dialog.className = 'device-profile-manager';
        this.dialog.innerHTML = `<div class="profile-manager-toolbar"><p>${t('profiles.intro')}</p><sl-button id="profile-new"><sl-icon slot="prefix" name="plus-lg"></sl-icon>${t('profiles.new')}</sl-button></div>
            <sl-alert id="profile-error" variant="danger" role="alert"></sl-alert>
            <div class="profile-manager-layout"><nav class="profile-manager-list" aria-label="${t('profiles.select')}"></nav><div class="profile-manager-editor"><p role="status">${t('profiles.loading')}</p></div></div>
            <sl-button slot="footer" id="profile-clone">${t('profiles.clone')}</sl-button><sl-button slot="footer" id="profile-close">${t('profiles.close')}</sl-button><sl-button slot="footer" variant="primary" id="profile-save">${t('profiles.save')}</sl-button>`;
        document.body.appendChild(this.dialog);
        this.dialog.querySelector('#profile-new').addEventListener('click', () => void this.startNew());
        this.dialog.querySelector('#profile-clone').addEventListener('click', () => void this.clone());
        this.dialog.querySelector('#profile-save').addEventListener('click', () => void this.save());
        this.dialog.querySelector('#profile-close').addEventListener('click', () => void this.close());
        this.dialog.addEventListener('sl-request-close', (event: Event) => { if (event.target !== this.dialog) return; event.preventDefault(); void this.close(); });
        this.dialog.addEventListener('sl-after-hide', (event: Event) => { if (event.target !== this.dialog) return; this.dialog.remove(); if (instance === this) instance = undefined; this.options.returnFocus?.focus(); });
        await customElements.whenDefined('sl-dialog'); await this.dialog.updateComplete; this.dialog.show();
        try {
            try { this.constraints = await rpcCall('device_profiles.constraints'); } catch { /* Older daemons still validate on save. */ }
            await this.reload();
            if (this.options.create) this.setProfile(newProfile(), true);
            else await this.select(this.options.selectedId ?? this.profiles[0]?.id);
        } catch (error) { this.error(error); }
    }
    private error(error: unknown, saved = false): void {
        const el = this.dialog.querySelector('#profile-error');
        el.textContent = `${t(saved ? 'profiles.saved_refresh_failed' : 'profiles.error')} ${error instanceof Error ? error.message : String(error)}`; el.open = true;
        if (this.profile?.id) {
            const reload = document.createElement('sl-button'); reload.setAttribute('size','small');
            reload.textContent = t('profiles.reload'); reload.addEventListener('click', () => void this.select(this.profile?.id));
            el.append(document.createElement('br'), reload);
        }
    }
    private async reload(): Promise<void> { this.profiles = await rpcCall('device_profiles.list'); this.renderList(); }
    private renderList(): void {
        const nav = this.dialog.querySelector('.profile-manager-list');
        nav.innerHTML = ['custom','system'].map(group => `<div class="profile-manager-group"><h3>${t(`profiles.${group}`)}</h3>${this.profiles.filter(p => (p.ownership === 'system' ? 'system' : 'custom') === group).map(displayProfile).map(p => `<button type="button" class="profile-list-item" data-profile="${escapeAttr(p.id)}" aria-current="${p.id === this.profile?.id}">${escapeAttr(p.name)}</button>`).join('')}</div>`).join('');
        nav.querySelectorAll('[data-profile]').forEach((button: HTMLElement) => button.addEventListener('click', () => void this.select(button.dataset.profile)));
    }
    private async permitDiscard(): Promise<boolean> {
        if (this.busy) return false;
        if (!this.draft || JSON.stringify(this.draft) === this.clean) return true;
        return new Promise(resolve => {
            const confirm: any = document.createElement('sl-dialog'); confirm.label = t('profiles.unsaved');
            confirm.innerHTML = `<p>${t('profiles.discard_help')}</p><sl-button slot="footer" id="profile-keep">${t('profiles.keep')}</sl-button><sl-button slot="footer" variant="danger" id="profile-discard">${t('profiles.discard')}</sl-button>`;
            document.body.appendChild(confirm);
            let discard = false;
            confirm.querySelector('#profile-keep').addEventListener('click', () => confirm.hide());
            confirm.querySelector('#profile-discard').addEventListener('click', () => { discard = true; confirm.hide(); });
            confirm.addEventListener('sl-after-hide', (event: Event) => { if (event.target === confirm) { confirm.remove(); resolve(discard); } });
            confirm.show();
        });
    }
    private async select(id?: string): Promise<void> {
        if (!id || !await this.permitDiscard()) { this.restorePicker(); return; }
        this.busy = true;
        let loaded = false;
        const controls = Array.from(this.dialog.querySelectorAll('.profile-editor-fields sl-input, .profile-editor-fields sl-textarea, .profile-editor-fields sl-select, .profile-editor-fields sl-checkbox, #profile-picker')) as any[];
        const disabled = controls.map(control => control.disabled);
        controls.forEach(control => { control.disabled = true; });
        try { const profile = await rpcCall('device_profiles.get', {id}); this.setProfile(profile, false); loaded = true; }
        catch (error) {
            controls.forEach((control,index) => { control.disabled = disabled[index]; });
            this.error(error); this.restorePicker();
        } finally { this.busy = false; if (loaded) this.renderEditor(); }
    }
    private restorePicker(): void { const picker = this.dialog.querySelector('#profile-picker'); if (picker) picker.value = this.profile?.id || '__draft__'; }
    private setProfile(profile: DeviceProfile, creating: boolean): void {
        this.numericTouched.clear(); this.serverErrors = {};
        this.profile = profile; this.draft = toDraft(profile); this.clean = JSON.stringify(this.draft); this.creating = creating;
        this.renderList(); this.renderEditor();
        const error = this.dialog?.querySelector('#profile-error'); if (error) error.open = false;
        const body = this.dialog?.shadowRoot?.querySelector('[part="body"]');
        if (body) body.scrollTop = 0;
    }
    private async startNew(): Promise<void> { if (await this.permitDiscard()) { this.cloneSource = undefined; this.setProfile(newProfile(), true); this.focus(); } }
    private async clone(): Promise<void> {
        if (!this.profile || !await this.permitDiscard()) return;
        const clone: DeviceProfile = JSON.parse(JSON.stringify(this.profile));
        this.cloneSource = this.profile.id;
        clone.id = ''; clone.name = t('profiles.copy_name', {name: clone.name}); clone.ownership = 'custom'; clone.revision = 0; delete clone.usage;
        this.setProfile(clone, true); this.clean = ''; this.focus();
    }
    private renderEditor(): void {
        if (!this.profile || !this.draft) return;
        const profile = this.profile, draft = this.draft, display = displayProfile(this.profile);
        const system = profile.ownership === 'system', unsupported = unsupportedRules(profile);
        const disabled = system || this.busy ? 'disabled' : '', audioDisabled = system || this.busy || unsupported.length ? 'disabled' : '';
        const input = (id: string, label: string, value: string, extra = '') => `<sl-input id="${id}" label="${escapeAttr(label)}" value="${escapeAttr(value)}" ${disabled} ${extra}></sl-input>`;
        this.dialog.querySelector('.profile-manager-editor').innerHTML = `
            <sl-select id="profile-picker" class="profile-manager-picker" ${this.busy ? 'disabled' : ''} label="${t('profiles.select')}" value="${escapeAttr(profile.id || '__draft__')}">${this.creating ? `<sl-option value="__draft__">${escapeAttr(profile.name)}</sl-option>` : ''}${profileOptions(this.profiles, false)}</sl-select>
            <div class="profile-editor-heading"><h2>${escapeAttr(display.name)}</h2><sl-badge variant="${system ? 'neutral' : 'primary'}">${t(system ? 'profiles.system' : 'profiles.custom')}</sl-badge></div>
            ${system ? `<p class="profile-help">${t('profiles.system_help')}</p>` : ''}
            ${unsupported.length ? `<sl-alert variant="warning" open>${escapeAttr(t('profiles.unsupported', {rules: unsupported.join(', ')}))}</sl-alert>` : ''}
            <div class="profile-editor-fields">${input('profile-name',t('profiles.name'),system ? display.name : draft.name,'required maxlength="120"')}
            <sl-textarea id="profile-description" label="${t('profiles.description')}" value="${escapeAttr(system ? display.description : draft.description)}" rows="2" maxlength="2000" ${disabled}></sl-textarea>
            <fieldset class="profile-formats" ${audioDisabled}><legend>${t('profiles.formats')}</legend>${formats.map(([id,label]) => `<sl-checkbox data-format="${id}" ${draft.formats.includes(id) ? 'checked' : ''} ${audioDisabled}>${label}</sl-checkbox>`).join('')}</fieldset>
            <sl-select id="profile-mode" label="${t('profiles.conversion')}" value="${draft.mode}" ${audioDisabled}><sl-option value="originals">${t('profiles.originals')}</sl-option><sl-option value="compatible">${t('profiles.compatible')}</sl-option><sl-option value="always">${t('profiles.always')}</sl-option></sl-select>
            <div class="profile-output-fields" ${draft.mode === 'originals' ? 'hidden' : ''}><sl-select id="profile-output" label="${t('profiles.output')}" value="${draft.output}" ${audioDisabled}>${outputs.map(([id,label]) => `<sl-option value="${id}">${label}</sl-option>`).join('')}</sl-select>
            ${input('profile-bitrate',t('profiles.bitrate'),draft.bitrate,`type="text" inputmode="decimal" ${audioDisabled} ${['flac','alac','wav'].includes(draft.output) ? 'hidden' : ''}`)}</div>
            <sl-details summary="${t('profiles.limits')}"><p class="profile-help">${t('profiles.unlimited_help')}</p><div class="profile-limit-fields">${limits.map(key => input(`profile-${key}`,t(`profiles.${key}`),draft.limits[key],`type="text" inputmode="${key === 'MaxStreamingBitrate' ? 'decimal' : 'numeric'}" placeholder="${t('profiles.unlimited')}" ${audioDisabled}`)).join('')}</div></sl-details>
            <sl-details summary="${t('profiles.folders')}"><p class="profile-help">${t('profiles.folders_help')}</p>${input('profile-music',t('basket.device.music_folder'),draft.music)}${input('profile-playlists',t('basket.device.playlist_folder'),draft.playlists)}${input('profile-audiobooks',t('basket.device.audiobook_folder'),draft.audiobooks,`placeholder="${escapeAttr(t('basket.device.media_folder_inherit'))}"`)}${input('profile-podcasts',t('basket.device.podcast_folder'),draft.podcasts,`placeholder="${escapeAttr(t('basket.device.media_folder_inherit'))}"`)}<sl-checkbox id="profile-artwork" ${draft.artwork ? 'checked' : ''} ${disabled} help-text="${escapeAttr(t('profiles.artwork_help'))}">${t('profiles.artwork')}</sl-checkbox></sl-details>
            <p class="profile-summary" aria-live="polite"></p>
            ${!system && profile.usage ? `<p class="profile-help">${escapeAttr(t('profiles.usage', {count: profile.usage.length, names: profile.usage.map(d => d.name || d.id).join(', ') || t('profiles.none')}))} ${t('profiles.offline_usage')}</p>` : ''}</div>`;
        this.dialog.querySelector('#profile-picker').addEventListener('sl-change', (event: any) => void this.select(event.target.value));
        const fields = this.dialog.querySelector('.profile-editor-fields');
        fields.addEventListener('sl-input', (event: Event) => { this.capture(); const id = (event.target as HTMLElement).id.replace('profile-', ''); this.numericTouched.add(id); delete this.serverErrors[id]; this.updateNumericFeedback(); });
        fields.addEventListener('sl-change', (event: Event) => {
            if (['profile-output','profile-mode'].includes((event.target as HTMLElement).id)) this.serverErrors = {};
            this.capture(); this.updateSummary(); this.updateNumericFeedback();
        });
        this.dialog.querySelector('#profile-save').hidden = system;
        this.dialog.querySelector('#profile-save').disabled = system || this.busy;
        this.dialog.querySelector('#profile-clone').disabled = this.creating || this.busy;
        this.dialog.querySelector('#profile-clone').textContent = t(system ? 'profiles.clone_customize' : 'profiles.clone');
        this.updateSummary(); this.updateNumericFeedback();
    }
    private capture(): void {
        if (!this.draft || this.busy) return;
        const value = (id: string) => this.dialog.querySelector(`#${id}`)?.value ?? '';
        Object.assign(this.draft, {name:value('profile-name'),description:value('profile-description'),music:value('profile-music'),playlists:value('profile-playlists'),audiobooks:value('profile-audiobooks'),podcasts:value('profile-podcasts'),artwork:this.dialog.querySelector('#profile-artwork')?.checked === true,mode:value('profile-mode'),output:value('profile-output'),bitrate:value('profile-bitrate')});
        this.draft.formats = formats.filter(([id]) => this.dialog.querySelector(`[data-format="${id}"]`)?.checked).map(([id]) => id);
        this.draft.limits = Object.fromEntries(limits.map(key => [key,value(`profile-${key}`)]));
    }
    private updateSummary(): void {
        if (!this.draft) return;
        const draft = this.draft, lossless = ['flac','alac','wav'].includes(draft.output);
        this.dialog.querySelector('.profile-summary').textContent = unsupportedRules(this.profile!).length ? t('profiles.summary_preserved') : draft.mode === 'originals' ? t('profiles.summary_originals') : t(draft.mode === 'always' ? 'profiles.summary_always' : 'profiles.summary_compatible', {formats:formats.filter(([id]) => draft.formats.includes(id)).map(f => f[1]).join(', ') || t('profiles.none'), output:outputs.find(f => f[0] === draft.output)?.[1] ?? draft.output, bitrate:lossless ? '' : `${draft.bitrate} kbps`});
        this.dialog.querySelector('.profile-output-fields').hidden = draft.mode === 'originals';
        this.dialog.querySelector('#profile-bitrate').hidden = lossless;
        this.dialog.querySelectorAll('[data-format]').forEach((el: any) => { el.disabled = this.busy || this.profile?.ownership === 'system' || !!unsupportedRules(this.profile!).length || draft.mode !== 'compatible'; });
    }
    private updateNumericFeedback(submit = false): boolean {
        if (!this.draft || !this.profile) return true;
        const errors = { ...profileNumericErrors(this.draft, this.profile, this.constraints), ...this.serverErrors };
        let first: any;
        for (const key of [...limits, 'bitrate']) {
            const control = this.dialog.querySelector(`#profile-${key}`);
            if (!control || control.disabled || this.draft.mode === 'originals' || (key === 'bitrate' && ['flac','alac','wav'].includes(this.draft.output))) { fieldFeedback(control); continue; }
            const rules = profileNumericRules(key, this.draft, this.constraints);
            const hint = key === 'MaxAudioSampleRate' ? t('numeric.hz_hint') : key === 'bitrate' || key === 'MaxStreamingBitrate' ? t('numeric.kbps_hint') : t('numeric.whole_hint', { min: rules.min ?? 0, max: rules.max ?? '' });
            fieldFeedback(control, submit || this.numericTouched.has(key) ? errors[key] : undefined, hint);
            if (errors[key]) first ??= control;
        }
        if (submit && first) revealInvalid(first);
        return !first;
    }
    private async save(): Promise<void> {
        if (this.busy || !this.profile || !this.draft || this.profile.ownership === 'system') return;
        this.capture(); if (!this.updateNumericFeedback(true)) return; const validation = validateDraft(this.draft, this.profile, this.constraints); if (validation) { this.error(validation); return; }
        const button = this.dialog.querySelector('#profile-save'); this.busy = true; button.loading = true;
        this.dialog.querySelectorAll('.profile-editor-fields sl-input, .profile-editor-fields sl-textarea, .profile-editor-fields sl-select, .profile-editor-fields sl-checkbox').forEach((control: any) => { control.disabled = true; });
        try {
            const profile = fromDraft(this.profile,this.draft);
            const saved = await rpcCall(this.creating ? this.cloneSource ? 'device_profiles.clone' : 'device_profiles.create' : 'device_profiles.update', {profile,revision:this.profile.revision,sourceId:this.cloneSource});
            // Commit the successful identity immediately: refresh failures must never retry creation.
            this.setProfile({...saved, usage:this.profile.usage ?? [], usageIncomplete:true},false);
            let refreshError: unknown;
            try {
                const details = await rpcCall('device_profiles.get', {id:saved.id});
                this.setProfile(details,false); await this.reload();
            } catch (error) { refreshError = error; }
            try { await this.options.onSaved?.(saved); } catch (error) { refreshError ??= error; }
            if (refreshError) this.error(refreshError,true);
            else this.dialog.querySelector('#profile-error').open = false;
        } catch (error) {
            const data = (error as any)?.data;
            if (data?.errorCode === 'PROFILE_NUMERIC_CONSTRAINT') {
                const key = data.field === 'MusicStreamingTranscodingBitrate' ? 'bitrate' : data.field;
                const bitrate = key === 'bitrate' || key === 'MaxStreamingBitrate';
                this.serverErrors[key] = key === 'MaxAudioSampleRate' ? t('numeric.sample_rate', { min: data.min, codec: this.draft.output.toUpperCase() }) : t('numeric.whole_hint', { min: data.min / (bitrate ? 1000 : 1), max: data.max / (bitrate ? 1000 : 1) });
                this.numericTouched.add(key);
            } else this.error(error);
        } finally { this.busy = false; button.loading = false; this.renderEditor(); this.updateNumericFeedback(true); }
    }
    private async close(): Promise<void> { if (this.closing || !await this.permitDiscard()) return; this.closing = true; this.dialog.hide(); }
}
