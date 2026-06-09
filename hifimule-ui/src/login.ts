import { rpcCall } from './rpc';
import { t } from './i18n';
import { SERVER_ICON_OPTIONS, defaultServerIcon, serverTypeLabel } from './serverIdentity';

type BadgeSpec = { label: string; variant: string };

function serverTypeBadge(type: string | null): BadgeSpec | null {
    switch (type) {
        case 'jellyfin':     return { label: serverTypeLabel(type), variant: 'primary' };
        case 'openSubsonic': return { label: serverTypeLabel(type), variant: 'success' };
        case 'subsonic':     return { label: serverTypeLabel(type), variant: 'neutral' };
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

function loginFormHtml(prefillUrl?: string, showIdentity = true): string {
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
            <div style="position: relative;">
                <sl-input name="url" label="${t('login.server_url')}" placeholder="${t('login.server_url_placeholder')}" ${urlAttrs} required></sl-input>
                <div id="server-type-indicator" style="min-height: 1.5rem; margin-top: 0.4rem;"></div>
            </div>
            <br>
            <sl-input name="username" label="${t('login.username')}" required></sl-input>
            <br>
            <sl-input name="password" type="password" label="${t('login.password')}" required password-toggle></sl-input>
            <br>
            ${identityFields}

            <div id="login-error" class="error-text" style="display: none; color: var(--sl-color-danger-500); margin-bottom: 1rem;"></div>

            <sl-button type="submit" variant="primary" style="width: 100%;">${t('login.connect')}</sl-button>
        </form>
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
        dialog.innerHTML = banner + loginFormHtml(options.prefillUrl, mode !== 'reauth');
        document.body.appendChild(dialog);
        dialog.addEventListener('sl-after-hide', (ev: Event) => {
            if (ev.target === dialog) {
                dialog.remove();
                options.onClose?.();
            }
        });
        customElements.whenDefined('sl-dialog').then(() => dialog.show());
        bindLoginForm(dialog, mode, () => {
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
                ${loginFormHtml(undefined, true)}
            </sl-card>
        </div>
    `;
    bindLoginForm(root as HTMLElement, mode, onLoginSuccess);
}

function bindLoginForm(root: HTMLElement, mode: NonNullable<LoginViewOptions['mode']>, onLoginSuccess: () => void) {
    const form = root.querySelector('#login-form') as HTMLFormElement;
    const indicator = root.querySelector('#server-type-indicator') as HTMLElement;
    const urlInput = form.querySelector('sl-input[name="url"]') as HTMLElement & { value: string };
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

    urlInput.addEventListener('sl-input', () => {
        if (probeTimer) clearTimeout(probeTimer);
        const url = urlInput.value.trim();
        if (!url.startsWith('http')) {
            indicator.innerHTML = '';
            applyIdentityDefaults('unknown');
            return;
        }
        probeTimer = setTimeout(async () => {
            try {
                const result = await rpcCall('server.probe', { url });
                const serverType = result?.serverType ?? null;
                const badge = serverTypeBadge(serverType);
                indicator.innerHTML = badge
                    ? `<sl-badge variant="${badge.variant}" pill>${badge.label}</sl-badge>`
                    : '';
                applyIdentityDefaults(serverType);
            } catch {
                indicator.innerHTML = '';
            }
        }, 600);
    });

    form.addEventListener('submit', async (e) => {
        e.preventDefault();
        const formData = new FormData(form);
        const url = formData.get('url') as string;
        const username = formData.get('username') as string;
        const password = formData.get('password') as string;
        const name = nameInput?.value?.trim();

        const btn = form.querySelector('sl-button') as HTMLElement & { loading: boolean };
        const errorEl = document.getElementById('login-error');

        if (btn) btn.loading = true;
        if (errorEl) errorEl.style.display = 'none';

        try {
            const payload: Record<string, string> = { url, serverType: 'auto', username, password };
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
