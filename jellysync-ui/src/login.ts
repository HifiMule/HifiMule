import { rpcCall } from './rpc';

export function initLoginView(onLoginSuccess: () => void) {
    console.log('Initializing Login View');
    // Actually, index.html has specific slots. We might need a main container.
    // index.html has <div class="app-container"> wrapping <sl-split-panel>.
    // We should probably replace the content of app-container or have a separate login container.
    // Let's assume we target formatting the body or a root div.

    // For now, let's target document.body or a specific #main-view
    const root = document.querySelector('.app-container');
    if (!root) return;

    root.innerHTML = `
        <div class="login-container">
            <sl-card class="login-card">
                <div slot="header">
                    <h3>Connect to Jellyfin</h3>
                </div>
                
                <form id="login-form" class="login-form">
                    <sl-input name="url" label="Server URL" placeholder="http://localhost:8096" required></sl-input>
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
    form.addEventListener('submit', async (e) => {
        e.preventDefault();
        const formData = new FormData(form);
        const url = formData.get('url') as string;
        const username = formData.get('username') as string;
        const password = formData.get('password') as string;

        // Shoelace button has a 'loading' property
        const btn = form.querySelector('sl-button') as HTMLElement & { loading: boolean };
        const errorEl = document.getElementById('login-error');

        if (btn) btn.loading = true;
        if (errorEl) errorEl.style.display = 'none';

        try {
            await rpcCall('login', { url, username, password });
            console.log('Login successful');
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
