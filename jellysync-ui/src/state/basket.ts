// Basket State Management
// Manages the collection of items selected for synchronization.

export interface BasketItem {
    id: string;
    name: string;
    type: string;
    artist?: string;
    childCount: number;
    sizeTicks: number; // cumulativeRunTimeTicks used for size estimation
}

class BasketStore extends EventTarget {
    private items: Map<string, BasketItem> = new Map();

    constructor() {
        super();
        this.loadFromLocalStorage();
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

    private notify() {
        this.dispatchEvent(new CustomEvent('update', { detail: this.getItems() }));
    }
}

export const basketStore = new BasketStore();
