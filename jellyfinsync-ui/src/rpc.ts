import { invoke } from '@tauri-apps/api/core';

export const RPC_PORT = (import.meta as any).env?.VITE_RPC_PORT || '19140';
export const RPC_URL = `http://localhost:${RPC_PORT}`;
export const IMAGE_PROXY_URL = `${RPC_URL}/jellyfin/image`;

export async function rpcCall(method: string, params: any = {}): Promise<any> {
    console.log(`RPC Call: ${method}`, params);
    // Use Tauri invoke to proxy RPC calls through the Rust backend.
    // Direct fetch from the webview to http://localhost is blocked in release mode
    // because Tauri serves pages from https://tauri.localhost (mixed content).
    return await invoke('rpc_proxy', { method, params });
}

/// Fetches a Jellyfin image via the Tauri backend, returning a data URL.
/// Works in both dev and release mode by bypassing browser mixed-content restrictions.
export async function getImageUrl(id: string, maxHeight?: number, quality?: number): Promise<string> {
    return await invoke('image_proxy', { id, maxHeight: maxHeight ?? null, quality: quality ?? null });
}
