import {
    audiobookshelfCancelSetup,
    audiobookshelfCommit,
    audiobookshelfDiscover,
    rpcCall,
    serverReauthenticate,
    type AudiobookshelfSetup,
} from './rpc';
import { t } from './i18n';
import { SERVER_ICON_OPTIONS, defaultServerIcon, serverTypeLabel } from './serverIdentity';
import {
    audiobookshelfRoleLabelKey,
    isLoginProviderChoice,
    shouldUseAudiobookshelfDiscovery,
    validLibraryChoices,
} from './audiobookshelfSetup';

type BadgeSpec = { label: string; variant: string };

function serverTypeBadge(type: string | null): BadgeSpec | null {
    switch (type) {
        case 'jellyfin':     return { label: serverTypeLabel(type), variant: 'primary' };
        case 'openSubsonic': return { label: serverTypeLabel(type), variant: 'success' };
        case 'subsonic':     return { label: serverTypeLabel(type), variant: 'neutral' };
        case 'audiobookshelf': return { label: 'Audiobookshelf', variant: 'warning' };
        default:             return null;
    }
}

function escapeHtml(text: string): string {
    return text
        .replace(/&/g, '&amp;')
        .replace(/</g, '&lt;')
        .replace(/>/g, '&gt;')
        .replace(/"/g, '&quot;');
}

export interface LoginViewOptions {
    /** 'first-run' takes over the whole window; 'add' / 'reauth' present the form
     * inline in a dialog without disrupting the current view (Story 2.11 AC4/AC11). */
    mode?: 'first-run' | 'add' | 'reauth';
    /** Pre-fills (and locks) the server URL — used by re-auth so the credential is
     * replaced only for that exact server (AC11). */
    prefillUrl?: string;
    /** Machine-local id and provider data used by scoped re-authentication. */
    serverId?: string;
    serverType?: string;
    prefillUsername?: string;
    /** Optional dialog title override. */
    dialogTitle?: string;
    /** Called when the inline dialog closes (success or cancel) — lets callers
     * reset any "prompt in flight" guard. */
    onClose?: () => void;
}

function iconPickerHtml(selectedIcon: string): string {
    return SERVER_ICON_OPTIONS.map(icon => `
        <button type="button" class="init-icon-tile ${icon === selectedIcon ? 'selected' : ''}" data-icon="${escapeHtml(icon)}" title="${escapeHtml(icon)}">
            <sl-icon name="${escapeHtml(icon)}"></sl-icon>
        </button>
    `).join('');
}

function loginFormHtml(options: LoginViewOptions, showIdentity = true): string {
    const prefillUrl = options.prefillUrl;
    const scopedReauth = options.mode === 'reauth' && options.serverType === 'audiobookshelf';
    const urlAttrs = prefillUrl
        ? `value="${prefillUrl.replace(/"/g, '&quot;')}" readonly`
        : '';
    const defaultIcon = defaultServerIcon('unknown');
    const identityFields = showIdentity ? `
            <div class="login-identity-fields">
                <sl-input name="serverName" label="${t('login.server_name')}" maxlength="40"></sl-input>
                <label class="device-settings-label">${t('login.server_icon')}</label>
                <div class="device-settings-icon-picker login-server-icon-picker">
                    ${iconPickerHtml(defaultIcon)}
                </div>
            </div>
            <br>
    ` : '';
    return `
        <form id="login-form" class="login-form">
            ${!scopedReauth ? `
            <sl-select name="serverType" label="${t('login.provider')}" value="auto" required>
                <sl-option value="auto">${t('login.provider_auto')}</sl-option>
                <sl-option value="jellyfin">Jellyfin</sl-option>
                <sl-option value="subsonic">Subsonic / OpenSubsonic</sl-option>
                <sl-option value="audiobookshelf">Audiobookshelf</sl-option>
            </sl-select>
            <br>` : ''}
            ${!scopedReauth ? `
            <div style="position: relative;">
                <sl-input name="url" label="${t('login.server_url')}" placeholder="${t('login.server_url_placeholder')}" ${urlAttrs} required></sl-input>
                <div id="server-type-indicator" style="min-height: 1.5rem; margin-top: 0.4rem;"></div>
            </div>
            <br>
            <sl-input name="username" label="${t('login.username')}" value="${escapeHtml(options.prefillUsername ?? '')}" required></sl-input>
            <br>` : `<p>${t('login.audiobookshelf.reauth_password_only')}</p>`}
            <sl-input name="password" type="password" label="${t('login.password')}" required password-toggle></sl-input>
            <br>
            ${identityFields}

            <div id="login-error" class="error-text" style="display: none; color: var(--sl-color-danger-500); margin-bottom: 1rem;"></div>

            <sl-button type="submit" variant="primary" style="width: 100%;">${t('login.connect')}</sl-button>
        </form>
        <div id="audiobookshelf-picker" hidden></div>
    `;
}

export function initLoginView(onLoginSuccess: () => void, options: LoginViewOptions = {}) {
    const mode = options.mode ?? 'first-run';
    console.log(`Initializing Login View (mode=${mode})`);

    let dialog: any = null;
    if (mode === 'add' || mode === 'reauth') {
        // Inline dialog — does not disrupt the current view (AC4 add / AC11 reauth).
        dialog = document.createElement('sl-dialog');
        dialog.label = options.dialogTitle ?? (mode === 'reauth' ? t('login.reauth_title') : t('serverHub.add'));
        const banner = mode === 'reauth'
            ? `<sl-alert variant="warning" open style="margin-bottom: 0.75rem;">
                 <sl-icon slot="icon" name="exclamation-triangle"></sl-icon>
                 ${t('login.reauth_hint')}
               </sl-alert>`
            : '';
        dialog.innerHTML = banner + loginFormHtml({ ...options, mode }, mode !== 'reauth');
        document.body.appendChild(dialog);
        dialog.addEventListener('sl-after-hide', (ev: Event) => {
            if (ev.target === dialog) {
                const setupId = dialog.querySelector('#audiobookshelf-picker')?.dataset.setupId;
                if (setupId) void audiobookshelfCancelSetup(setupId).catch(() => undefined);
                dialog.remove();
                options.onClose?.();
            }
        });
        customElements.whenDefined('sl-dialog').then(() => dialog.show());
        bindLoginForm(dialog, mode, options, () => {
            dialog.hide();
            onLoginSuccess();
        });
        return;
    }

    const root = document.querySelector('.app-container');
    if (!root) return;

    root.innerHTML = `
        <div class="login-container">
            <sl-card class="login-card">
                <div slot="header">
                    <h3>${t('login.title')}</h3>
                </div>
                ${loginFormHtml({ ...options, mode }, true)}
            </sl-card>
        </div>
    `;
    bindLoginForm(root as HTMLElement, mode, options, onLoginSuccess);
}

function bindLoginForm(
    root: HTMLElement,
    mode: NonNullable<LoginViewOptions['mode']>,
    options: LoginViewOptions,
    onLoginSuccess: () => void,
) {
    const form = root.querySelector('#login-form') as HTMLFormElement;
    const indicator = root.querySelector('#server-type-indicator') as HTMLElement | null;
    const urlInput = form.querySelector('sl-input[name="url"]') as (HTMLElement & { value: string }) | null;
    const providerSelect = form.querySelector('sl-select[name="serverType"]') as (HTMLElement & { value: string }) | null;
    const nameInput = form.querySelector('sl-input[name="serverName"]') as (HTMLElement & { value: string }) | null;
    const identityEnabled = mode !== 'reauth' && Boolean(nameInput);

    let probeTimer: ReturnType<typeof setTimeout> | null = null;
    let selectedIcon = defaultServerIcon('unknown');
    let lastDefaultName = '';
    let nameEdited = false;
    let iconEdited = false;

    const setSelectedIcon = (icon: string) => {
        selectedIcon = icon;
        root.querySelectorAll('.login-server-icon-picker .init-icon-tile').forEach(tile => {
            tile.classList.toggle('selected', (tile as HTMLElement).dataset.icon === icon);
        });
    };

    root.querySelectorAll('.login-server-icon-picker .init-icon-tile').forEach(tile => {
        tile.addEventListener('click', () => {
            const icon = (tile as HTMLElement).dataset.icon;
            if (!icon) return;
            iconEdited = true;
            setSelectedIcon(icon);
        });
    });

    nameInput?.addEventListener('sl-input', () => {
        nameEdited = true;
    });

    const applyIdentityDefaults = (serverType: string | null) => {
        if (!identityEnabled || !serverType) return;
        const nextName = serverTypeLabel(serverType);
        const nextIcon = defaultServerIcon(serverType);
        if (nameInput && (!nameEdited || nameInput.value.trim() === '' || nameInput.value === lastDefaultName)) {
            nameInput.value = nextName;
            nameEdited = false;
        }
        if (!iconEdited) {
            setSelectedIcon(nextIcon);
        }
        lastDefaultName = nextName;
    };

    providerSelect?.addEventListener('sl-change', () => {
        const provider = providerSelect.value;
        if (!isLoginProviderChoice(provider)) return;
        if (indicator) {
            const badge = serverTypeBadge(provider === 'auto' ? null : provider);
            indicator.innerHTML = badge
                ? `<sl-badge variant="${badge.variant}" pill>${badge.label}</sl-badge>`
                : '';
        }
        applyIdentityDefaults(provider === 'auto' ? 'unknown' : provider);
    });

    urlInput?.addEventListener('sl-input', () => {
        if (probeTimer) clearTimeout(probeTimer);
        const url = urlInput.value.trim();
        if (providerSelect?.value !== 'auto') return;
        if (!url.startsWith('http')) {
            if (indicator) indicator.innerHTML = '';
            applyIdentityDefaults('unknown');
            return;
        }
        probeTimer = setTimeout(async () => {
            try {
                const result = await rpcCall('server.probe', { url });
                const serverType = result?.serverType ?? null;
                const badge = serverTypeBadge(serverType);
                if (indicator) {
                    indicator.innerHTML = badge
                        ? `<sl-badge variant="${badge.variant}" pill>${badge.label}</sl-badge>`
                        : '';
                }
                applyIdentityDefaults(serverType);
            } catch {
                if (indicator) indicator.innerHTML = '';
            }
        }, 600);
    });

    form.addEventListener('submit', async (e) => {
        e.preventDefault();
        const formData = new FormData(form);
        const url = (formData.get('url') as string | null) ?? options.prefillUrl ?? '';
        const username = (formData.get('username') as string | null) ?? options.prefillUsername ?? '';
        const password = formData.get('password') as string;
        const name = nameInput?.value?.trim();

        const btn = form.querySelector('sl-button') as HTMLElement & { loading: boolean };
        const errorEl = root.querySelector('#login-error') as HTMLElement | null;

        if (btn) btn.loading = true;
        if (errorEl) errorEl.style.display = 'none';

        try {
            if (mode === 'reauth' && options.serverType === 'audiobookshelf') {
                if (!options.serverId) throw new Error(t('login.audiobookshelf.reauth_missing_server'));
                await serverReauthenticate(options.serverId, password);
                onLoginSuccess();
                return;
            }

            const selectedProvider = providerSelect?.value ?? 'auto';
            if (!isLoginProviderChoice(selectedProvider)) {
                throw new Error(t('login.provider_required'));
            }
            const detectedProvider = selectedProvider === 'auto'
                ? (await rpcCall('server.probe', { url }))?.serverType ?? null
                : selectedProvider;
            if (shouldUseAudiobookshelfDiscovery(selectedProvider, detectedProvider)) {
                const setup = await audiobookshelfDiscover({
                    url,
                    username: username.trim(),
                    password,
                });
                renderAudiobookshelfPicker(
                    root,
                    setup,
                    { name: nameEdited ? name : undefined, icon: iconEdited ? selectedIcon : undefined },
                    onLoginSuccess,
                );
                return;
            }

            const payload: Record<string, string> = { url, serverType: selectedProvider, username, password };
            if (identityEnabled && name) {
                payload.name = name;
                payload.icon = selectedIcon;
            }
            await rpcCall('server.connect', payload);
            console.log('Server connection successful');
            onLoginSuccess();
        } catch (err: any) {
            console.error('Login failed', err);
            if (errorEl) {
                errorEl.textContent = err.message || t('login.authentication_failed');
                errorEl.style.display = 'block';
            }
        } finally {
            if (btn) btn.loading = false;
        }
    });
}

function renderAudiobookshelfPicker(
    root: HTMLElement,
    setup: AudiobookshelfSetup,
    identity: { name?: string; icon?: string },
    onLoginSuccess: () => void,
): void {
    const form = root.querySelector('#login-form') as HTMLElement | null;
    const picker = root.querySelector('#audiobookshelf-picker') as HTMLElement | null;
    if (!form || !picker) return;
    const libraries = validLibraryChoices(setup.libraries);
    form.hidden = true;
    picker.hidden = false;
    picker.dataset.setupId = setup.setupId;
    if (libraries.length === 0) {
        picker.innerHTML = `<sl-alert variant="warning" open>${t('login.audiobookshelf.empty')}</sl-alert>`;
        return;
    }
    picker.innerHTML = `
        <h3>${t('login.audiobookshelf.choose_library')}</h3>
        <p>${t('login.audiobookshelf.choose_library_hint')}</p>
        <sl-radio-group name="audiobookshelf-library" label="${t('login.audiobookshelf.library_label')}" required>
            ${libraries.map(choice => `
                <sl-radio value="${escapeHtml(choice.choiceId)}">
                    <span>${escapeHtml(choice.name)}</span>
                    <sl-badge pill variant="neutral">${t(audiobookshelfRoleLabelKey(choice.role))}</sl-badge>
                </sl-radio>
            `).join('')}
        </sl-radio-group>
        <div id="audiobookshelf-picker-error" class="error-text" role="alert" style="display:none"></div>
        <div style="display:flex;gap:.5rem;justify-content:flex-end;margin-top:1rem">
            <sl-button id="audiobookshelf-picker-back" variant="default">${t('login.audiobookshelf.back')}</sl-button>
            <sl-button id="audiobookshelf-picker-commit" variant="primary">${t('login.audiobookshelf.add_library')}</sl-button>
        </div>
    `;
    const group = picker.querySelector('sl-radio-group') as (HTMLElement & { value: string }) | null;
    const back = picker.querySelector('#audiobookshelf-picker-back') as (HTMLElement & { disabled: boolean }) | null;
    const commit = picker.querySelector('#audiobookshelf-picker-commit') as (HTMLElement & { loading: boolean }) | null;
    const error = picker.querySelector('#audiobookshelf-picker-error') as HTMLElement | null;
    back?.addEventListener('click', async () => {
        await audiobookshelfCancelSetup(setup.setupId).catch(() => undefined);
        delete picker.dataset.setupId;
        picker.hidden = true;
        picker.innerHTML = '';
        form.hidden = false;
    });
    commit?.addEventListener('click', async () => {
        const choiceId = group?.value;
        if (!choiceId || !libraries.some(choice => choice.choiceId === choiceId)) {
            if (error) {
                error.textContent = t('login.audiobookshelf.choice_required');
                error.style.display = 'block';
            }
            return;
        }
        try {
            commit.loading = true;
            if (back) back.disabled = true;
            if (error) error.style.display = 'none';
            await audiobookshelfCommit({ setupId: setup.setupId, choiceId, ...identity });
            delete picker.dataset.setupId;
            onLoginSuccess();
        } catch (caught) {
            if (error) {
                error.textContent = caught instanceof Error ? caught.message : t('login.authentication_failed');
                error.style.display = 'block';
            }
        } finally {
            commit.loading = false;
            if (back) back.disabled = false;
        }
    });
    customElements.whenDefined('sl-radio-group').then(() => {
        (picker.querySelector('sl-radio') as HTMLElement | null)?.focus();
    });
}
