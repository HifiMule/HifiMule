import { rpcCall } from './rpc';

type BadgeSpec = { label: string; variant: string };

function serverTypeBadge(type: string | null): BadgeSpec | null {
    switch (type) {
        case 'jellyfin':     return { label: 'Jellyfin',     variant: 'primary' };
        case 'openSubsonic': return { label: 'OpenSubsonic', variant: 'success' };
        case 'subsonic':     return { label: 'Subsonic',     variant: 'neutral' };
        default:             return null;
    }
}

export function initLoginView(onLoginSuccess: () => void) {
    console.log('Initializing Login View');
    const root = document.querySelector('.app-container');
    if (!root) return;

    root.innerHTML = `
        <div class="login-container">
            <sl-card class="login-card">
                <div slot="header">
                    <h3>Connect to Media Server</h3>
                </div>

                <form id="login-form" class="login-form">
                    <div style="position: relative;">
                        <sl-input name="url" label="Server URL" placeholder="http://localhost:4533 or http://localhost:8096" required></sl-input>
                        <div id="server-type-indicator" style="min-height: 1.5rem; margin-top: 0.4rem;"></div>
                    </div>
                    <br>
                    <sl-input name="username" label="Username" required></sl-input>
                    <br>
                    <sl-input name="password" type="password" label="Password" required password-toggle></sl-input>
                    <br>

                    <div id="login-error" class="error-text" style="display: none; color: var(--sl-color-danger-500); margin-bottom: 1rem;"></div>

                    <sl-button type="submit" variant="primary" style="width: 100%;">Connect</sl-button>
                </form>
            </sl-card>
        </div>
    `;

    const form = document.getElementById('login-form') as HTMLFormElement;
    const indicator = document.getElementById('server-type-indicator')!;
    const urlInput = form.querySelector('sl-input[name="url"]') as HTMLElement & { value: string };

    let probeTimer: ReturnType<typeof setTimeout> | null = null;

    urlInput.addEventListener('sl-input', () => {
        if (probeTimer) clearTimeout(probeTimer);
        const url = urlInput.value.trim();
        if (!url.startsWith('http')) {
            indicator.innerHTML = '';
            return;
        }
        probeTimer = setTimeout(async () => {
            try {
                const result = await rpcCall('server.probe', { url });
                const badge = serverTypeBadge(result?.serverType ?? null);
                indicator.innerHTML = badge
                    ? `<sl-badge variant="${badge.variant}" pill>${badge.label}</sl-badge>`
                    : '';
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

        const btn = form.querySelector('sl-button') as HTMLElement & { loading: boolean };
        const errorEl = document.getElementById('login-error');

        if (btn) btn.loading = true;
        if (errorEl) errorEl.style.display = 'none';

        try {
            await rpcCall('server.connect', { url, serverType: 'auto', username, password });
            console.log('Server connection successful');
            onLoginSuccess();
        } catch (err: any) {
            console.error('Login failed', err);
            if (errorEl) {
                errorEl.textContent = err.message || 'Authentication failed';
                errorEl.style.display = 'block';
            }
        } finally {
            if (btn) btn.loading = false;
        }
    });
}
