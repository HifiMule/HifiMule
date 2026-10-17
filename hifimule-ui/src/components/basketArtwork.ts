/** One observer per basket; bounded cache survives redraws but never provider changes. */
export class BasketArtwork {
    private observer?: IntersectionObserver;
    private generation = 0;
    private serverId: string | null | undefined;
    private cache = new Map<string, Promise<string>>();

    constructor(private load: (id: string, height: number, quality: number) => Promise<string>) {}

    disconnect(): void {
        this.generation++;
        this.observer?.disconnect();
        this.observer = undefined;
    }

    destroy(): void {
        this.disconnect();
        this.cache.clear();
    }

    mount(container: HTMLElement, serverId: string | null): void {
        this.disconnect();
        if (serverId !== this.serverId) {
            this.cache.clear();
            this.serverId = serverId;
        }
        const generation = this.generation;
        const start = (element: HTMLElement) => {
            if (generation !== this.generation || !element.isConnected) return;
            const id = element.dataset.imageId;
            if (!id) return;
            this.observer?.unobserve(element);
            let request = this.cache.get(id);
            if (!request) {
                request = this.load(id, 100, 80);
                this.cache.set(id, request);
                if (this.cache.size > 128) this.cache.delete(this.cache.keys().next().value!);
            }
            void request.then(url => {
                if (generation === this.generation && element.isConnected) {
                    element.style.backgroundImage = `url('${url}')`;
                }
            }).catch(() => {
                if (this.cache.get(id) === request) this.cache.delete(id);
            });
        };
        if (typeof IntersectionObserver !== 'undefined') {
            this.observer = new IntersectionObserver(entries => {
                for (const entry of entries) if (entry.isIntersecting) start(entry.target as HTMLElement);
            });
        }
        for (const element of container.querySelectorAll<HTMLElement>('.basket-item-image[data-image-id]')) {
            if (this.observer) this.observer.observe(element);
            else start(element);
        }
    }
}
