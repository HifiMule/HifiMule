// Basket Sidebar Component
// Displays the list of items selected for synchronization.

import { basketStore, BasketItem } from '../state/basket';
import { IMAGE_PROXY_URL } from '../rpc';

export class BasketSidebar {
    private container: HTMLElement;
    private updateListener: () => void;
    private isDestroyed: boolean = false;

    constructor(container: HTMLElement) {
        this.container = container;
        this.updateListener = () => this.render();
        this.init();
    }

    private init() {
        basketStore.addEventListener('update', this.updateListener);
        this.render();
    }

    public destroy() {
        this.isDestroyed = true;
        basketStore.removeEventListener('update', this.updateListener);
    }

    public render() {
        if (this.isDestroyed) return;

        const items = basketStore.getItems();

        if (items.length === 0) {
            this.container.innerHTML = `
                <div class="basket-header">
                    <h2>Basket</h2>
                    <sl-badge variant="neutral" pill>0</sl-badge>
                </div>
                <div class="basket-placeholder">
                    <sl-icon name="basket" style="font-size: 2rem; opacity: 0.5;"></sl-icon>
                    <p style="opacity: 0.5;">Your basket is empty</p>
                </div>
                <div class="basket-footer">
                     <sl-button variant="primary" style="width: 100%;" disabled>
                        <sl-icon slot="prefix" name="cloud-download"></sl-icon>
                        Start Sync
                    </sl-button>
                </div>
            `;
            return;
        }

        const totalTracks = items.reduce((sum, item) => sum + item.childCount, 0);

        this.container.innerHTML = `
            <div class="basket-header">
                <h2>Basket</h2>
                <sl-badge variant="primary" pill>${items.length}</sl-badge>
            </div>
            
            <div class="basket-items-list">
                ${items.map(item => this.renderItem(item)).join('')}
            </div>

            <div class="basket-footer">
                <div class="basket-summary">
                    <span>Total: <strong>${totalTracks} tracks</strong></span>
                </div>
                <sl-button variant="primary" style="width: 100%;">
                    <sl-icon slot="prefix" name="cloud-download"></sl-icon>
                    Start Sync
                </sl-button>
                <sl-button variant="text" size="small" class="clear-basket-btn" style="width: 100%; margin-top: 0.5rem;">
                    Clear All
                </sl-button>
            </div>
        `;

        // Bind events
        this.container.querySelectorAll('.remove-item-btn').forEach(btn => {
            btn.addEventListener('click', (e) => {
                const id = (e.currentTarget as HTMLElement).getAttribute('data-id');
                if (id) basketStore.remove(id);
            });
        });

        this.container.querySelector('.clear-basket-btn')?.addEventListener('click', () => {
            basketStore.clear();
        });
    }

    private renderItem(item: BasketItem): string {
        // We reuse the image proxy from the daemon
        const imageUrl = `${IMAGE_PROXY_URL}/${item.id}?maxHeight=100&quality=80`;

        return `
            <div class="basket-item-card" data-id="${item.id}">
                <div class="basket-item-image" style="background-image: url('${imageUrl}')"></div>
                <div class="basket-item-info">
                    <div class="basket-item-name">${this.escapeHtml(item.name)}</div>
                    <div class="basket-item-meta">${item.childCount} tracks • ${item.type}</div>
                </div>
                <sl-icon-button name="x" class="remove-item-btn" data-id="${item.id}" label="Remove"></sl-icon-button>
            </div>
        `;
    }

    private escapeHtml(text: string): string {
        const div = document.createElement('div');
        div.textContent = text;
        return div.innerHTML;
    }
}
