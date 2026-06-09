import { rpcCall } from '../rpc';

// Basket State Management
// Manages the collection of items selected for synchronization.

export const AUTO_FILL_SLOT_ID = '__auto_fill_slot__';

export interface BasketItem {
    id: string;
    name: string;
    type: string;
    serverId?: string;
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
    private activeServerId: string | null = null;

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

    public setActiveServerId(serverId: string | null) {
        if (this.activeServerId === serverId) return;
        this.activeServerId = serverId;
        // Story 2.11 (AC3): switching servers no longer deletes other-server items.
        // They are retained and rendered read-only; only the selected server changes.
        this.notify();
    }

    public getActiveServerId(): string | null {
        return this.activeServerId;
    }

    /** True when an item belongs to a server other than the selected one and must
     * render read-only/locked (AC3/AC35). Items with no serverId are editable. */
    public isItemLocked(item: BasketItem): boolean {
        return (
            !!item.serverId &&
            !!this.activeServerId &&
            item.serverId !== this.activeServerId
        );
    }

    /** True when the basket holds items from more than one distinct server. */
    public hasMultipleServers(): boolean {
        const servers = new Set<string>();
        for (const item of this.items.values()) {
            if (item.id !== AUTO_FILL_SLOT_ID && item.serverId) servers.add(item.serverId);
        }
        return servers.size > 1;
    }

    /** Distinct serverIds present among non-auto-fill items, in insertion order. */
    public serverIdsInBasket(): string[] {
        const out: string[] = [];
        for (const item of this.items.values()) {
            if (item.id !== AUTO_FILL_SLOT_ID && item.serverId && !out.includes(item.serverId)) {
                out.push(item.serverId);
            }
        }
        return out;
    }

    /** Removes all items belonging to a removed server (AC7). Returns the count. */
    public removeItemsForServer(serverId: string): number {
        let removed = 0;
        for (const [id, item] of this.items) {
            if (item.serverId === serverId) {
                this.items.delete(id);
                removed++;
            }
        }
        if (removed > 0) {
            this._dirty = true;
            this.saveToLocalStorage();
            this.saveBasketToDaemon();
            this.notify();
        }
        return removed;
    }

    /** Reconciles persisted items keyed by a legacy serverId — the pre-2.11 composite
     * (`type|url|username`) OR the 2.11 machine-local UUID — onto the deterministic
     * PORTABLE server id (Story 2.13), so an upgrade does not strand or lock the
     * existing localStorage basket. Idempotent: items already tagged with a portable
     * id are not remapped (never maps portable → anything). */
    public reconcileServerIds(
        servers: Array<{ id: string; serverId?: string | null; serverType: string; url: string; username: string }>
    ): void {
        if (servers.length === 0) return;
        const remap = new Map<string, string>();
        for (const s of servers) {
            if (!s.serverId) continue; // no portable id known yet — nothing to map to
            const normalizedUrl = s.url.trim().replace(/\/+$/, '').toLowerCase();
            // pre-2.11 composite → portable
            remap.set(`${s.serverType}|${normalizedUrl}|${s.username}`, s.serverId);
            // 2.11 machine-local UUID → portable
            remap.set(s.id, s.serverId);
        }
        let changed = false;
        for (const item of this.items.values()) {
            if (item.serverId && remap.has(item.serverId)) {
                const portable = remap.get(item.serverId)!;
                if (item.serverId !== portable) {
                    item.serverId = portable;
                    changed = true;
                }
            }
        }
        if (changed) {
            this.saveToLocalStorage();
            this.notify();
        }
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

        // Device manifest is the source of truth — replace local state entirely.
        // Strip the virtual auto-fill slot: it is runtime-only and must not be
        // restored from a stale persisted state.
        this.items.clear();
        for (const item of items) {
            // Retain items from ALL servers (AC3); the daemon already reconciled
            // serverIds. Read-only rendering for non-selected servers is the view's job.
            if (item.id !== AUTO_FILL_SLOT_ID) {
                this.items.set(item.id, item);
            }
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

    public clearLocalOnly() {
        if (this.saveTimeout !== null) {
            window.clearTimeout(this.saveTimeout);
            this.saveTimeout = null;
        }
        this.items.clear();
        this._dirty = false;
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
            const saved = localStorage.getItem('hifimule-basket');
            if (saved) {
                const parsed = JSON.parse(saved);
                // Strip the virtual auto-fill slot on load — it is runtime-only.
                this.items = new Map(
                    Object.entries(parsed).filter(([id]) => id !== AUTO_FILL_SLOT_ID) as [string, BasketItem][]
                );
            }

            const dirty = localStorage.getItem('hifimule-basket-dirty');
            this._dirty = dirty === 'true';
        } catch (e) {
            console.error("Failed to load basket from localStorage:", e);
        }
    }

    private saveToLocalStorage() {
        const obj = Object.fromEntries(this.items);
        localStorage.setItem('hifimule-basket', JSON.stringify(obj));
        localStorage.setItem('hifimule-basket-dirty', this._dirty.toString());
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
        if (!this.activeServerId) {
            window.dispatchEvent(new CustomEvent('toast', { detail: { type: 'error', message: 'Connect to a server before adding items' } }));
            return;
        }
        item.serverId = this.activeServerId;
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

    /** Returns only the IDs of manually added items (for exclude list in auto-fill). */
    public getManualItemIds(): string[] {
        return Array.from(this.items.values())
            .filter(i => i.id !== AUTO_FILL_SLOT_ID)
            .map(i => i.id);
    }

    /** Returns total size of manually added items only (excludes the virtual auto-fill slot). */
    public getManualSizeBytes(): number {
        let total = 0;
        for (const item of this.items.values()) {
            if (item.id !== AUTO_FILL_SLOT_ID) total += item.sizeBytes ?? 0;
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
