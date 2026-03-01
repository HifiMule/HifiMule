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

    public hydrateFromDaemon(items: BasketItem[]) {
        if (this._syncingFromDaemon) return;
        this._syncingFromDaemon = true;

        // MD5 or similar check would be better, but let's compare IDs for now
        const daemonIds = new Set(items.map(i => i.id));
        const localIds = new Set(this.items.keys());

        let mismatch = daemonIds.size !== localIds.size;
        if (!mismatch) {
            for (const id of daemonIds) {
                if (!localIds.has(id)) {
                    mismatch = true;
                    break;
                }
            }
        }

        if (mismatch) {
            console.log("Basket mismatch detected during hydration, setting dirty flag.");
            this._dirty = true;
        }

        // Merge: keep local selections, add daemon selections
        // This prevents clobbering if the user rapidly adds items before hydration completes (F2)
        const currentItems = new Map(this.items);
        this.items.clear();
        for (const item of items) {
            this.items.set(item.id, item);
        }
        for (const [id, item] of currentItems) {
            this.items.set(id, item);
        }
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
            const saved = localStorage.getItem('jellysync-basket');
            if (saved) {
                const parsed = JSON.parse(saved);
                this.items = new Map(Object.entries(parsed));
            }

            const dirty = localStorage.getItem('jellysync-basket-dirty');
            this._dirty = dirty === 'true';
        } catch (e) {
            console.error("Failed to load basket from localStorage:", e);
        }
    }

    private saveToLocalStorage() {
        const obj = Object.fromEntries(this.items);
        localStorage.setItem('jellysync-basket', JSON.stringify(obj));
        localStorage.setItem('jellysync-basket-dirty', this._dirty.toString());
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
