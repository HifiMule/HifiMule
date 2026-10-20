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

/** A known split always travels with its full capacity reservation. Optional
 * fields keep older daemon totals authoritative without inventing a split. */
export interface SelectionSize {
    selectedBytes: number;
    expectedBytes?: number;
    reserveBytes?: number;
}

export function selectionSize(selectedBytes: number, expectedBytes?: number, reserveBytes?: number): SelectionSize {
    const valid = (value: number | undefined): value is number => typeof value === 'number' && Number.isFinite(value) && value >= 0;
    const total = valid(selectedBytes) ? selectedBytes : 0;
    const sum = Math.min((expectedBytes ?? NaN) + (reserveBytes ?? NaN), 18_446_744_073_709_551_615);
    const tolerance = Number.isSafeInteger(total) ? 0 : total * Number.EPSILON;
    return valid(expectedBytes) && valid(reserveBytes) && Math.abs(sum - total) <= tolerance
        ? { selectedBytes: total, expectedBytes, reserveBytes }
        : { selectedBytes: total };
}

/** Coarse profile reservation until preparation resolves actual direct/reused files.
 * Unknown duration reserves an hour per track; never substitute original bytes.
 * Match the daemon's payload + one percent + 64 KiB per converted track policy.
 */
export function plannedSelectionSize(items: {sizeBytes:number;sizeTicks:number;childCount:number;type?:string}[], targetKbps?:number|null): SelectionSize {
    const bounded = (value: number) => Number.isFinite(value) ? Math.min(Math.max(value, 0), Number.MAX_SAFE_INTEGER) : value > 0 ? Number.MAX_SAFE_INTEGER : 0;
    let expectedBytes = 0, reserveBytes = 0;
    for (const item of items) {
        const tracks = Math.max(item.childCount || 1, 1);
        const converted = targetKbps != null && !['Book', 'BookPart', 'PodcastEpisode', 'PodcastShow'].includes(item.type ?? '');
        const payload = !converted ? bounded(item.sizeBytes)
            : bounded(Math.ceil((item.sizeTicks > 0 ? item.sizeTicks / 10_000_000 : 3600 * tracks) * targetKbps! * 125));
        expectedBytes = bounded(expectedBytes + payload);
        reserveBytes = bounded(reserveBytes + (!converted ? 0 : Math.ceil(payload / 100) + 65_536 * tracks));
    }
    reserveBytes = Math.min(reserveBytes, Number.MAX_SAFE_INTEGER - expectedBytes);
    return { selectedBytes: expectedBytes + reserveBytes, expectedBytes, reserveBytes };
}

export function plannedSelectionBytes(items: {sizeBytes:number;sizeTicks:number;childCount:number;type?:string}[], targetKbps?:number|null): number {
    return plannedSelectionSize(items, targetKbps).selectedBytes;
}
