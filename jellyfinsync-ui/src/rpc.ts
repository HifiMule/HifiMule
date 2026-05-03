import { invoke } from '@tauri-apps/api/core';

export const RPC_PORT = (import.meta as any).env?.VITE_RPC_PORT || '19140';
export const RPC_URL = `http://localhost:${RPC_PORT}`;
export const IMAGE_PROXY_URL = `${RPC_URL}/jellyfin/image`;

function getErrorMessage(error: unknown): string {
    if (error instanceof Error && error.message.trim()) {
        return error.message;
    }

    if (typeof error === 'string' && error.trim()) {
        return error;
    }

    if (error && typeof error === 'object') {
        const record = error as Record<string, unknown>;
        for (const key of ['message', 'error', 'details']) {
            const value = record[key];
            if (typeof value === 'string' && value.trim()) {
                return value;
            }
        }

        try {
            const serialized = JSON.stringify(error);
            if (serialized && serialized !== '{}') {
                return serialized;
            }
        } catch {
            // Fall through to generic message.
        }
    }

    return 'Unknown RPC error';
}

export async function rpcCall(method: string, params: any = {}): Promise<any> {
    console.log(`RPC Call: ${method}`, params);
    // Use Tauri invoke to proxy RPC calls through the Rust backend.
    // Direct fetch from the webview to http://localhost is blocked in release mode
    // because Tauri serves pages from https://tauri.localhost (mixed content).
    try {
        return await invoke('rpc_proxy', { method, params });
    } catch (error) {
        throw new Error(getErrorMessage(error));
    }
}

/// Fetches a Jellyfin image via the Tauri backend, returning a data URL.
/// Works in both dev and release mode by bypassing browser mixed-content restrictions.
export async function getImageUrl(id: string, maxHeight?: number, quality?: number): Promise<string> {
    return await invoke('image_proxy', { id, maxHeight: maxHeight ?? null, quality: quality ?? null });
}
