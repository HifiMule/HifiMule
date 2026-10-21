import type { BasketItem } from './basket';

export interface BasketItemEstimate {
    id: string;
    serverId?: string;
    childCount: number;
    sizeTicks: number;
    sizeBytes: number;
    plannedBytes: number;
    expectedBytes?: number;
    reserveBytes?: number;
}

export interface BasketEstimate {
    targetDeviceId: string;
    selectedBytes: number;
    expectedBytes?: number;
    reserveBytes?: number;
    trackCount: number;
    items: BasketItemEstimate[];
}

/** Ignore display metadata and Auto-Fill slots: repairing a stale duration or
 * polling free space must not expand a large library again. Resident usage is
 * refreshed after completed syncs, when last_sync_at changes. */
export function basketEstimateKey(device: any, serverId: string | null, items: BasketItem[], force = false): string {
    return JSON.stringify([
        device?.device_id ?? device?.deviceId, serverId,
        device?.transcoding_profile_id ?? device?.transcodingProfileId,
        device?.plannedProfileRevision,
        device?.transcoding_profile_dirty ?? device?.transcodingProfileDirty,
        device?.last_synced_transcoding_profile_id ?? device?.lastSyncedTranscodingProfileId,
        device?.last_sync_at ?? device?.lastSyncAt, device?.reconcile_before_sync ?? device?.reconcileBeforeSync, device?.folderArtwork,
        force,
        items.filter(item => !item.id.startsWith('__auto_fill_slot__'))
            .map(item => [item.serverId ?? serverId, item.id, item.type]),
    ]);
}

/** A bounded, single-selection cache. Superseded requests may finish but cannot
 * publish their result into the current selection or overwrite its pending job. */
export class BasketEstimateCache {
    private key: string | null = null;
    private result: BasketEstimate | undefined;
    private pending: Promise<BasketEstimate | undefined> | undefined;
    private retryAfter = 0;

    get(key: string): BasketEstimate | undefined {
        return this.key === key ? this.result : undefined;
    }

    clear(): void {
        this.key = null;
        this.result = undefined;
        this.pending = undefined;
        this.retryAfter = 0;
    }

    async load(key: string, fetch: () => Promise<BasketEstimate>): Promise<BasketEstimate | undefined> {
        if (key !== this.key) {
            this.clear();
            this.key = key;
        }
        if (this.result) return this.result;
        if (this.pending) return this.pending;
        if (Date.now() < this.retryAfter) return undefined;
        const job: Promise<BasketEstimate | undefined> = Promise.resolve().then(fetch).then(result => {
            if (this.key === key && this.pending === job) this.result = result;
            return this.key === key && this.pending === job ? result : undefined;
        }).catch(error => {
            if (this.key === key && this.pending === job) this.retryAfter = Date.now() + 30_000;
            throw error;
        }).finally(() => {
            if (this.key === key && this.pending === job) this.pending = undefined;
        });
        this.pending = job;
        return job;
    }
}
