import { rpcCall } from '../rpc';

// Basket State Management
// Manages the collection of items selected for synchronization.

export interface BasketItem {
    id: string;
    name: string;
    type: string;
    artist?: string;
    childCount: number;
    sizeTicks: number; // cumulativeRunTimeTicks used for duration display
    sizeBytes: number; // actual file size from MediaSources
    autoFilled?: boolean;    // true when added by the auto-fill algorithm
    priorityReason?: string; // "favorite" | "playCount:N" | "new"
}

class BasketStore extends EventTarget {
    private items: Map<string, BasketItem> = new Map();
    private _syncingFromDaemon: boolean = false;
    private _dirty: boolean = false;

    constructor() {
        super();
        this.loadFromLocalStorage();
        this.hydrate();
    }

    private saveTimeout: number | null = null;
    private async saveBasketToDaemon() {
        if (this._syncingFromDaemon) return;
        if (this.saveTimeout !== null) {
            window.clearTimeout(this.saveTimeout);
        }
        this.saveTimeout = window.setTimeout(async () => {
            this.saveTimeout = null;
            try {
                await rpcCall('manifest_save_basket', { basketItems: this.getItems() });
            } catch (e) {
                console.error("Failed to save basket to daemon:", e);
                window.dispatchEvent(new CustomEvent('toast', { detail: { type: 'error', message: 'Failed to save basket to device' } }));
            }
        }, 1000);
    }

    /** Immediately flush any pending debounced basket save to the daemon.
     * Must be called before switching devices so the current device's basket
     * is persisted before the selection changes. */
    public async flushPendingSave(): Promise<void> {
        if (this.saveTimeout !== null) {
            window.clearTimeout(this.saveTimeout);
            this.saveTimeout = null;
            // Let errors propagate — the caller (device switch) must not proceed
            // if the current device's basket could not be persisted.
            await rpcCall('manifest_save_basket', { basketItems: this.getItems() });
        }
    }

    public hydrateFromDaemon(items: BasketItem[]) {
        if (this._syncingFromDaemon) return;
        this._syncingFromDaemon = true;

        // Device manifest is the source of truth — replace local state entirely
        this.items.clear();
        for (const item of items) {
            this.items.set(item.id, item);
        }
        this._dirty = false;
        this.saveToLocalStorage();
        this.notify();
        this._syncingFromDaemon = false;
    }

    public clearForDevice() {
        this.items.clear();
        this.saveToLocalStorage();
        this.notify();
    }

    private async hydrate() {
        const missingSizeIds: string[] = [];
        for (const item of this.items.values()) {
            if (item.sizeBytes === undefined) {
                missingSizeIds.push(item.id);
            }
        }

        if (missingSizeIds.length > 0) {
            console.log(`Hydrating sizes for ${missingSizeIds.length} items...`);
            try {
                const sizes = await rpcCall('jellyfin_get_item_sizes', { itemIds: missingSizeIds });

                let updated = false;
                for (const sizeInfo of sizes) {
                    const item = this.items.get(sizeInfo.id);
                    if (item) {
                        item.sizeBytes = sizeInfo.totalSizeBytes;
                        updated = true;
                    }
                }

                if (updated) {
                    this.saveToLocalStorage();
                    this.notify();
                    console.log('Hydration complete.');
                }
            } catch (e) {
                console.error("Failed to hydrate basket item sizes:", e);
            }
        }
    }

    private loadFromLocalStorage() {
        try {
            const saved = localStorage.getItem('jellyfinsync-basket');
            if (saved) {
                const parsed = JSON.parse(saved);
                this.items = new Map(Object.entries(parsed));
            }

            const dirty = localStorage.getItem('jellyfinsync-basket-dirty');
            this._dirty = dirty === 'true';
        } catch (e) {
            console.error("Failed to load basket from localStorage:", e);
        }
    }

    private saveToLocalStorage() {
        const obj = Object.fromEntries(this.items);
        localStorage.setItem('jellyfinsync-basket', JSON.stringify(obj));
        localStorage.setItem('jellyfinsync-basket-dirty', this._dirty.toString());
    }

    public isDirty(): boolean {
        return this._dirty;
    }

    public resetDirty() {
        this._dirty = false;
        this.saveToLocalStorage();
        this.notify();
    }

    public getItems(): BasketItem[] {
        return Array.from(this.items.values());
    }

    public has(id: string): boolean {
        return this.items.has(id);
    }

    public add(item: BasketItem) {
        this.items.set(item.id, item);
        this._dirty = true;
        this.saveToLocalStorage();
        this.saveBasketToDaemon();
        this.notify();
    }

    public remove(id: string) {
        if (this.items.delete(id)) {
            this._dirty = true;
            this.saveToLocalStorage();
            this.saveBasketToDaemon();
            this.notify();
        }
    }

    public toggle(item: BasketItem) {
        if (this.has(item.id)) {
            this.remove(item.id);
        } else {
            this.add(item);
        }
    }

    public clear() {
        this.items.clear();
        this._dirty = true;
        this.saveToLocalStorage();
        this.saveBasketToDaemon();
        this.notify();
    }

    /** Replace all auto-filled items with the new set, preserving manual items. */
    public replaceAutoFilled(autoFilledItems: BasketItem[]) {
        // Collect auto-filled IDs in a first pass, then delete — avoids mutating the
        // Map while iterating it (ECMAScript spec §24.1.3.5 leaves deletion of
        // not-yet-visited entries implementation-defined).
        const autoFilledIds: string[] = [];
        for (const [id, item] of this.items) {
            if (item.autoFilled) autoFilledIds.push(id);
        }
        for (const id of autoFilledIds) {
            this.items.delete(id);
        }
        // Add new auto-filled items (after manual items in insertion order).
        // Never overwrite a manually added item with an auto-fill entry — if the daemon
        // returns a track that the user already added manually, skip it.
        for (const item of autoFilledItems) {
            const existing = this.items.get(item.id);
            if (existing && !existing.autoFilled) continue;
            this.items.set(item.id, { ...item, autoFilled: true });
        }
        this._dirty = true;
        this.saveToLocalStorage();
        this.saveBasketToDaemon();
        this.notify();
    }

    /** Returns only the IDs of manually added items (for exclude list in auto-fill). */
    public getManualItemIds(): string[] {
        return Array.from(this.items.values())
            .filter(i => !i.autoFilled)
            .map(i => i.id);
    }

    /** Returns total size of manually added items only. */
    public getManualSizeBytes(): number {
        let total = 0;
        for (const item of this.items.values()) {
            if (!item.autoFilled) total += item.sizeBytes ?? 0;
        }
        return total;
    }

    public getTotalSizeBytes(): number {
        let total = 0;
        for (const item of this.items.values()) {
            total += item.sizeBytes || 0;
        }
        return total;
    }

    private notify() {
        this.dispatchEvent(new CustomEvent('update', { detail: this.getItems() }));
    }
}

export const basketStore = new BasketStore();
