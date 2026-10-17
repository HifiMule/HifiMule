/** Observe artwork only while its card is mounted. Detached library pages must
 * not keep observers alive or request covers from a subsequently selected server. */
export class CardArtwork extends HTMLElement {
    load?: () => Promise<unknown>;
    private observer?: IntersectionObserver;
    private started = false;

    connectedCallback(): void {
        if (this.started) return;
        const start = () => {
            if (!this.isConnected || this.started || !this.load) return;
            this.started = true;
            this.observer?.disconnect();
            this.observer = undefined;
            void this.load();
        };
        if (typeof IntersectionObserver === 'undefined') {
            queueMicrotask(start);
            return;
        }
        this.observer = new IntersectionObserver(entries => {
            if (entries.some(entry => entry.isIntersecting)) start();
        });
        this.observer.observe(this);
    }

    disconnectedCallback(): void {
        this.observer?.disconnect();
        this.observer = undefined;
    }
}

if (!customElements.get('hm-card-artwork')) {
    customElements.define('hm-card-artwork', CardArtwork);
}
