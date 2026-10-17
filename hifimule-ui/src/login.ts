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
        <button type="button" class="init-icon-tile ${icon === selectedIcon ? 'selected' : ''}" data-icon="${escapeHtml(icon)}" title="${escapeHtml(icon)}" aria-label="${escapeHtml(icon)}" aria-pressed="${icon === selectedIcon}">
            <sl-icon name="${escapeHtml(icon)}"></sl-icon>
        </button>
    `).join('');
}

function loginFormHtml(options: LoginViewOptions, showIdentity = true): string {
    const prefillUrl = options.prefillUrl;
    const scopedReauth = options.mode === 'reauth' && options.serverType === 'audiobookshelf';
    const urlAttrs = prefillUrl
        ? `value="${escapeHtml(prefillUrl)}" readonly`
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
            ` : ''}
            ${!scopedReauth ? `
            <div style="position: relative;">
                <sl-input name="url" label="${t('login.server_url')}" placeholder="${t('login.server_url_placeholder')}" ${urlAttrs} required></sl-input>
                <div id="server-type-indicator" aria-live="polite"></div>
            </div>
            <sl-input name="username" label="${t('login.username')}" value="${escapeHtml(options.prefillUsername ?? '')}" required></sl-input>
            ` : `<p>${t('login.audiobookshelf.reauth_password_only')}</p>`}
            <sl-input name="password" type="password" label="${t('login.password')}" required password-toggle></sl-input>
            ${identityFields}

            <div id="login-error" class="error-text" role="alert" tabindex="-1" style="display: none; color: var(--sl-color-danger-500); margin-bottom: 1rem;"></div>

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
                    <h1>${t('login.title')}</h1>
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
            const selected = (tile as HTMLElement).dataset.icon === icon;
            tile.classList.toggle('selected', selected);
            tile.setAttribute('aria-pressed', String(selected));
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

    let probeGeneration = 0;
    let submitting = false;
    const invalidateProbe = () => {
        probeGeneration += 1;
        if (probeTimer) clearTimeout(probeTimer);
    };
    root.addEventListener('sl-hide', (event) => {
        if (event.target === root) invalidateProbe();
    });
    // A committed connection must finish before its dialog can be dismissed.
    root.addEventListener('sl-request-close', (event) => {
        if (event.target === root && submitting) event.preventDefault();
    });
    providerSelect?.addEventListener('sl-change', () => {
        probeGeneration += 1;
        if (probeTimer) clearTimeout(probeTimer);
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
        probeGeneration += 1;
        if (probeTimer) clearTimeout(probeTimer);
        const url = urlInput.value.trim();
        if (providerSelect?.value !== 'auto') return;
        if (indicator) indicator.innerHTML = '';
        if (!url.startsWith('http')) {
            if (indicator) indicator.innerHTML = '';
            applyIdentityDefaults('unknown');
            return;
        }
        const generation = probeGeneration;
        probeTimer = setTimeout(async () => {
            if (!root.isConnected || submitting) return;
            try {
                const result = await rpcCall('server.probe', { url });
                if (generation !== probeGeneration || providerSelect?.value !== 'auto') return;
                if (!root.isConnected) return;
                const serverType = result?.serverType ?? null;
                const badge = serverTypeBadge(serverType);
                if (indicator) {
                    indicator.innerHTML = badge
                        ? `<sl-badge variant="${badge.variant}" pill>${badge.label}</sl-badge>`
                        : '';
                }
                applyIdentityDefaults(serverType);
            } catch {
                if (generation !== probeGeneration || !root.isConnected) return;
                if (indicator) indicator.innerHTML = '';
            }
        }, 600);
    });

    form.addEventListener('submit', async (e) => {
        e.preventDefault();
        if (submitting || form.hidden) return;
        submitting = true;
        invalidateProbe();
        const formData = new FormData(form);
        const url = ((formData.get('url') as string | null) ?? options.prefillUrl ?? '').trim();
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
                errorEl.textContent = err?.message || (typeof err === 'string' && err) || t('login.authentication_failed');
                errorEl.style.display = 'block';
                errorEl.focus();
            }
        } finally {
            submitting = false;
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
    picker.innerHTML = `
        ${libraries.length === 0 ? `<sl-alert variant="warning" open>${t('login.audiobookshelf.empty')}</sl-alert>` : `
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
        `}
        <div id="audiobookshelf-picker-error" class="error-text" role="alert" style="display:none"></div>
        <div class="login-picker-actions">
            <sl-button id="audiobookshelf-picker-back" variant="default">${t('login.audiobookshelf.back')}</sl-button>
            ${libraries.length ? `<sl-button id="audiobookshelf-picker-commit" variant="primary">${t('login.audiobookshelf.add_library')}</sl-button>` : ''}
        </div>
    `;
    const group = picker.querySelector('sl-radio-group') as (HTMLElement & { value: string }) | null;
    const back = picker.querySelector('#audiobookshelf-picker-back') as (HTMLElement & { disabled: boolean }) | null;
    const commit = picker.querySelector('#audiobookshelf-picker-commit') as (HTMLElement & { loading: boolean }) | null;
    const error = picker.querySelector('#audiobookshelf-picker-error') as HTMLElement | null;
    let busy = false;
    const preventClose = (event: Event) => {
        if (event.target === root && busy) event.preventDefault();
    };
    root.addEventListener('sl-request-close', preventClose);
    back?.addEventListener('click', async () => {
        if (busy) return;
        busy = true;
        back.disabled = true;
        await audiobookshelfCancelSetup(setup.setupId).catch(() => undefined);
        root.removeEventListener('sl-request-close', preventClose);
        delete picker.dataset.setupId;
        picker.hidden = true;
        picker.innerHTML = '';
        form.hidden = false;
        (form.querySelector('sl-input[name="url"]') as HTMLElement | null)?.focus();
    });
    commit?.addEventListener('click', async () => {
        if (busy) return;
        const choiceId = group?.value;
        if (!choiceId || !libraries.some(choice => choice.choiceId === choiceId)) {
            if (error) {
                error.textContent = t('login.audiobookshelf.choice_required');
                error.style.display = 'block';
            }
            return;
        }
        try {
            busy = true;
            commit.loading = true;
            if (back) back.disabled = true;
            if (error) error.style.display = 'none';
            await audiobookshelfCommit({ setupId: setup.setupId, choiceId, ...identity });
            delete picker.dataset.setupId;
            onLoginSuccess();
        } catch (caught) {
            delete picker.dataset.setupId;
            picker.hidden = true;
            picker.innerHTML = '';
            form.hidden = false;
            const formError = root.querySelector('#login-error') as HTMLElement | null;
            if (formError) {
                formError.textContent = caught instanceof Error ? caught.message : t('login.authentication_failed');
                formError.style.display = 'block';
                formError.focus();
            }
        } finally {
            busy = false;
            root.removeEventListener('sl-request-close', preventClose);
            commit.loading = false;
            if (back) back.disabled = false;
        }
    });
    customElements.whenDefined('sl-radio-group').then(() => {
        ((picker.querySelector('sl-radio') as HTMLElement | null) ?? back)?.focus();
    });
}
