import { t } from '../i18n';

const statuses = new WeakMap<HTMLElement, LibraryLoadStatus>();

/** A sibling survives content replacement and can announce while content is busy. */
export function libraryLoadStatus(content: HTMLElement): LibraryLoadStatus {
    let status = statuses.get(content);
    if (!status) {
        status = new LibraryLoadStatus(content);
        statuses.set(content, status);
    }
    return status;
}

class LibraryLoadStatus {
    private region = document.createElement('div');
    private pending = false;
    private revision = 0;

    constructor(private content: HTMLElement) {
        this.region.className = 'sr-only';
        this.region.setAttribute('role', 'status');
        this.region.setAttribute('aria-live', 'polite');
        this.region.setAttribute('aria-atomic', 'true');
        content.before(this.region);
    }

    start(): void {
        this.content.setAttribute('aria-busy', 'true');
        if (this.pending) return;
        this.pending = true;
        this.announce(t('library.loading.pending'));
    }

    complete(count?: number): void {
        if (!this.pending) return;
        this.pending = false;
        this.content.setAttribute('aria-busy', 'false');
        this.announce(t(count === undefined ? 'library.loading.ready'
            : count === 0 ? 'library.loading.empty' : 'library.loading.complete', { count: count ?? 0 }));
    }

    fail(): void {
        this.pending = false;
        this.content.setAttribute('aria-busy', 'false');
        this.announce(t('library.loading.failed'));
    }

    private announce(message: string): void {
        const revision = ++this.revision;
        this.region.textContent = '';
        // Insert an empty live region first, then mutate it. Suppress obsolete
        // messages from fast loads and detached views without moving focus.
        requestAnimationFrame(() => {
            if (revision === this.revision && this.content.isConnected && this.region.isConnected) {
                this.region.textContent = message;
            }
        });
    }
}
