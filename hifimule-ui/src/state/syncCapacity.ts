export interface SyncStorage {
    totalBytes: number;
    freeBytes: number;
    usedBytes: number;
}

export interface SyncedCapacityItem {
    sizeBytes: number;
    isAutoFill?: boolean;
}

/** The full selected basket replaces managed files; autofill ceilings are never demand. */
export function syncCapacity(storage: SyncStorage | null, selectedBytes: number, items: SyncedCapacityItem[] = []) {
    if (!storage) return null;
    const syncedSelectedBytes = items.filter(item => !item.isAutoFill).reduce((sum, item) => sum + item.sizeBytes, 0);
    const syncedAutofillBytes = items.filter(item => item.isAutoFill).reduce((sum, item) => sum + item.sizeBytes, 0);
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
