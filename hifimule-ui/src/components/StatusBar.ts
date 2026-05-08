// Status Bar Component
// Shows daemon connection state, RPC info, device status, and errors at the bottom of the app.

import { RPC_URL } from '../rpc';

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
        daemonState: 'Unknown',
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
            this.state.daemonState = 'Disconnected';
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
                        this.state.daemonState = 'Not logged in';
                    } else if (r.activeOperationId) {
                        this.state.daemonState = 'Syncing';
                    } else if (r.currentDevice) {
                        this.state.daemonState = r.currentDevice.dirty ? 'Device (dirty)' : 'Idle';
                    } else {
                        this.state.daemonState = 'Idle';
                    }

                    // Device name from mapping or manifest
                    this.state.deviceName = r.deviceMapping?.name
                        || r.currentDevice?.name
                        || (r.pendingDevicePath ? 'Unrecognized device' : null);
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
            this.state.daemonState = 'Unreachable';
            this.state.lastError = e.message || 'Connection failed';
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
        const statusText = connected ? 'Connected' : 'Disconnected';

        const deviceSection = this.state.deviceName
            ? `<span class="statusbar-device" title="Connected device">
                 <sl-icon name="usb-drive"></sl-icon> ${this.escapeHtml(this.state.deviceName)}
               </span>`
            : `<span class="statusbar-device statusbar-dim">
                 <sl-icon name="usb-drive"></sl-icon> No device
               </span>`;

        const errorSection = this.state.lastError
            ? `<span class="statusbar-error" title="${this.escapeHtml(this.state.lastError)}">
                 <sl-icon name="exclamation-triangle"></sl-icon> ${this.escapeHtml(this.truncate(this.state.lastError, 60))}
               </span>`
            : '';

        const lastRpc = this.state.lastRpcMethod
            ? `<span class="statusbar-rpc statusbar-dim" title="Last RPC call">
                 ${this.escapeHtml(this.state.lastRpcMethod)} ${this.state.lastRpcTime ? this.formatAge(this.state.lastRpcTime) : ''}
               </span>`
            : '';

        this.container.innerHTML = `
            <div class="statusbar">
                <div class="statusbar-left">
                    <span class="statusbar-connection" title="Daemon: ${RPC_URL}">
                        <span class="statusbar-dot" style="background: ${dotColor};"></span>
                        Daemon: ${statusText}
                    </span>
                    <span class="statusbar-state statusbar-dim">${this.escapeHtml(this.state.daemonState)}</span>
                    ${deviceSection}
                </div>
                <div class="statusbar-right">
                    ${errorSection}
                    ${lastRpc}
                    <span class="statusbar-url statusbar-dim" title="RPC endpoint">${RPC_URL}</span>
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
        if (seconds < 5) return 'just now';
        if (seconds < 60) return `${seconds}s ago`;
        return `${Math.floor(seconds / 60)}m ago`;
    }

    destroy() {
        if (this.pollInterval) clearInterval(this.pollInterval);
        if (this.errorTimeout) clearTimeout(this.errorTimeout);
    }
}
