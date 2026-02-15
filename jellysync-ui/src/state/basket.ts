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

    constructor() {
        super();
        this.loadFromLocalStorage();
        this.hydrate();
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
        } catch (e) {
            console.error("Failed to load basket from localStorage:", e);
        }
    }

    private saveToLocalStorage() {
        const obj = Object.fromEntries(this.items);
        localStorage.setItem('jellysync-basket', JSON.stringify(obj));
    }

    public getItems(): BasketItem[] {
        return Array.from(this.items.values());
    }

    public has(id: string): boolean {
        return this.items.has(id);
    }

    public add(item: BasketItem) {
        this.items.set(item.id, item);
        this.saveToLocalStorage();
        this.notify();
    }

    public remove(id: string) {
        if (this.items.delete(id)) {
            this.saveToLocalStorage();
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
        this.saveToLocalStorage();
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
