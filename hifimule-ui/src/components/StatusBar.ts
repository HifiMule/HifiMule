// Status Bar Component
// Shows daemon connection state, RPC info, device status, and errors at the bottom of the app.

import { RPC_URL } from '../rpc';
import { t } from '../i18n';

export interface StatusBarState {
    daemonConnected: boolean;
    daemonState: string;
    deviceName: string | null;
    lastError: string | null;
    lastRpcMethod: string | null;
    lastRpcTime: number | null;
}

export class StatusBar {
    private container: HTMLElement;
    private state: StatusBarState = {
        daemonConnected: false,
        daemonState: t('ui.status.unknown'),
        deviceName: null,
        lastError: null,
        lastRpcMethod: null,
        lastRpcTime: null,
    };
    private pollInterval: ReturnType<typeof setInterval> | null = null;
    private errorTimeout: ReturnType<typeof setTimeout> | null = null;

    constructor(container: HTMLElement) {
        this.container = container;
        this.render();
        this.startPolling();
        this.listenForRpcEvents();
    }

    private listenForRpcEvents() {
        window.addEventListener('rpc:call', ((e: CustomEvent) => {
            this.state.lastRpcMethod = e.detail.method;
            this.state.lastRpcTime = Date.now();
            this.render();
        }) as EventListener);

        window.addEventListener('rpc:success', (() => {
            if (!this.state.daemonConnected) {
                this.state.daemonConnected = true;
                this.state.lastError = null;
                this.render();
            }
        }) as EventListener);

        window.addEventListener('rpc:error', ((e: CustomEvent) => {
            this.state.lastError = e.detail.error;
            this.render();
            // Clear transient errors after 10 seconds
            if (this.errorTimeout) clearTimeout(this.errorTimeout);
            this.errorTimeout = setTimeout(() => {
                if (this.state.lastError === e.detail.error) {
                    this.state.lastError = null;
                    this.render();
                }
            }, 10000);
        }) as EventListener);

        window.addEventListener('rpc:disconnect', (() => {
            this.state.daemonConnected = false;
            this.state.daemonState = t('ui.status.disconnected');
            this.render();
        }) as EventListener);
    }

    private async pollDaemonState() {
        try {
            const response = await fetch(RPC_URL, {
                method: 'POST',
                headers: { 'Content-Type': 'application/json' },
                body: JSON.stringify({
                    jsonrpc: '2.0',
                    method: 'get_daemon_state',
                    params: {},
                    id: Date.now()
                })
            });

            if (response.ok) {
                const data = await response.json();
                const wasDisconnected = !this.state.daemonConnected;
                this.state.daemonConnected = true;

                if (data.result) {
                    const r = data.result;
                    // Derive daemon state from response fields
                    if (r.serverConnected === false) {
                        this.state.daemonState = t('ui.status.not_logged_in');
                    } else if (r.activeOperationId) {
                        this.state.daemonState = t('ui.status.syncing');
                    } else if (r.currentDevice) {
                        this.state.daemonState = r.currentDevice.dirty ? t('ui.status.device_dirty') : t('ui.status.idle');
                    } else {
                        this.state.daemonState = t('ui.status.idle');
                    }

                    // Device name from mapping or manifest
                    this.state.deviceName = r.deviceMapping?.name
                        || r.currentDevice?.name
                        || (r.pendingDevicePath ? t('ui.status.unrecognized_device') : null);
                }

                if (wasDisconnected) {
                    this.state.lastError = null;
                }
                this.render();
            } else {
                this.state.daemonConnected = false;
                this.state.daemonState = `HTTP ${response.status}`;
                this.render();
            }
        } catch (e: any) {
            this.state.daemonConnected = false;
            this.state.daemonState = t('ui.status.unreachable');
            this.state.lastError = e.message || t('ui.status.connection_failed');
            this.render();
        }
    }

    private startPolling() {
        // Initial poll
        this.pollDaemonState();
        // Poll every 3 seconds
        this.pollInterval = setInterval(() => this.pollDaemonState(), 3000);
    }

    private render() {
        const connected = this.state.daemonConnected;
        const dotColor = connected ? '#22c55e' : '#ef4444';
        const statusText = connected ? t('ui.status.connected') : t('ui.status.disconnected');

        const deviceSection = this.state.deviceName
            ? `<span class="statusbar-device" title="${t('ui.status.connected_device')}">
                 <sl-icon name="usb-drive"></sl-icon> ${this.escapeHtml(this.state.deviceName)}
               </span>`
            : `<span class="statusbar-device statusbar-dim">
                 <sl-icon name="usb-drive"></sl-icon> ${t('ui.status.no_device')}
               </span>`;

        const errorSection = this.state.lastError
            ? `<span class="statusbar-error" title="${this.escapeHtml(this.state.lastError)}">
                 <sl-icon name="exclamation-triangle"></sl-icon> ${this.escapeHtml(this.truncate(this.state.lastError, 60))}
               </span>`
            : '';

        const lastRpc = this.state.lastRpcMethod
            ? `<span class="statusbar-rpc statusbar-dim" title="${t('ui.status.last_rpc_call')}">
                 ${this.escapeHtml(this.state.lastRpcMethod)} ${this.state.lastRpcTime ? this.formatAge(this.state.lastRpcTime) : ''}
               </span>`
            : '';

        this.container.innerHTML = `
            <div class="statusbar">
                <div class="statusbar-left">
                    <span class="statusbar-connection" title="${t('ui.status.daemon')}: ${RPC_URL}">
                        <span class="statusbar-dot" style="background: ${dotColor};"></span>
                        ${t('ui.status.daemon')}: ${statusText}
                    </span>
                    <span class="statusbar-state statusbar-dim">${this.escapeHtml(this.state.daemonState)}</span>
                    ${deviceSection}
                </div>
                <div class="statusbar-right">
                    ${errorSection}
                    ${lastRpc}
                    <span class="statusbar-url statusbar-dim" title="${t('ui.status.rpc_endpoint')}">${RPC_URL}</span>
                </div>
            </div>
        `;
    }

    private escapeHtml(text: string): string {
        const div = document.createElement('div');
        div.textContent = text;
        return div.innerHTML;
    }

    private truncate(text: string, max: number): string {
        return text.length > max ? text.slice(0, max) + '...' : text;
    }

    private formatAge(timestamp: number): string {
        const seconds = Math.floor((Date.now() - timestamp) / 1000);
        if (seconds < 5) return t('ui.time.just_now');
        if (seconds < 60) return t('ui.time.seconds_ago', { count: seconds });
        return t('ui.time.minutes_ago', { count: Math.floor(seconds / 60) });
    }

    destroy() {
        if (this.pollInterval) clearInterval(this.pollInterval);
        if (this.errorTimeout) clearTimeout(this.errorTimeout);
    }
}
