export const RPC_PORT = (import.meta as any).env?.VITE_RPC_PORT || '19140';
export const RPC_URL = `http://localhost:${RPC_PORT}`;
export const IMAGE_PROXY_URL = `${RPC_URL}/jellyfin/image`;

export async function rpcCall(method: string, params: any = {}): Promise<any> {
    console.log(`RPC Call: ${method}`, params);
    const response = await fetch(RPC_URL, {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({
            jsonrpc: '2.0',
            method,
            params,
            id: Date.now()
        })
    });

    const data = await response.json();
    if (data.error) {
        console.error(`RPC Error: ${method}`, data.error);
        throw new Error(data.error.message);
    }
    return data.result;
}
