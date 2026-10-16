export interface SyncStorage {
    totalBytes: number;
    freeBytes: number;
    usedBytes: number;
}

export interface SyncedCapacityItem {
    sizeBytes: number;
    isAutoFill?: boolean;
}

export interface SyncedCapacityTotals { selectedBytes: number; autofillBytes: number; }

/** The full selected basket replaces managed files; autofill ceilings are never demand. */
export function syncCapacity(storage: SyncStorage | null, selectedBytes: number, items: SyncedCapacityItem[] | SyncedCapacityTotals = []) {
    if (!storage) return null;
    const syncedSelectedBytes = Array.isArray(items) ? items.filter(item => !item.isAutoFill).reduce((sum, item) => sum + item.sizeBytes, 0) : items.selectedBytes;
    const syncedAutofillBytes = Array.isArray(items) ? items.filter(item => item.isAutoFill).reduce((sum, item) => sum + item.sizeBytes, 0) : items.autofillBytes;
    const managedBytes = syncedSelectedBytes + syncedAutofillBytes;
    const availableBytes = storage.freeBytes + managedBytes;
    const remainingBytes = availableBytes - selectedBytes;
    const unmanagedUsedBytes = Math.max(storage.usedBytes - managedBytes, 0);
    const usedFraction = storage.totalBytes > 0 ? Math.min(unmanagedUsedBytes / storage.totalBytes, 1) : 0;
    const selectedFraction = storage.totalBytes > 0 ? Math.min(selectedBytes / storage.totalBytes, 1 - usedFraction) : 0;
    return {
        syncedSelectedBytes, syncedAutofillBytes, availableBytes, remainingBytes,
        autofillCapacityBytes: Math.max(remainingBytes, 0),
        overBytes: Math.max(-remainingBytes, 0),
        zone: remainingBytes < 0 ? 'red' : remainingBytes < storage.totalBytes * 0.1 ? 'amber' : 'green',
        usedFraction, selectedFraction, freeFraction: Math.max(1 - usedFraction - selectedFraction, 0),
    } as const;
}

/** Coarse profile reservation until preparation resolves actual direct/reused files.
 * Unknown duration reserves an hour per track; never substitute original bytes.
 */
export function plannedSelectionBytes(items: {sizeBytes:number;sizeTicks:number;childCount:number}[], targetKbps?:number|null): number {
    return items.reduce((sum,item) => sum + (targetKbps == null ? item.sizeBytes :
        Math.ceil((item.sizeTicks > 0 ? item.sizeTicks / 10_000_000 : 3600 * Math.max(item.childCount,1)) * targetKbps * 125 * 1.03) + 65_536 * Math.max(item.childCount,1)),0);
}
