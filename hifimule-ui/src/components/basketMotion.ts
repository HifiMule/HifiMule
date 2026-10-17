const segments = ['capacity-used', 'capacity-projected', 'capacity-autofill', 'capacity-free'];
const rows = '.basket-item-card[data-id]';

/** Capture the visible frame before the sidebar replaces its DOM. */
export function captureBasketMotion(container: HTMLElement) {
    return {
        ids: new Set(Array.from(container.querySelectorAll<HTMLElement>(rows), row => row.dataset.id)),
        transforms: new Map(segments.flatMap(name => {
            const element = container.querySelector<HTMLElement>(`.${name}`);
            return element ? [[name, getComputedStyle(element).transform] as const] : [];
        })),
    };
}

export function animateBasketUpdate(container: HTMLElement, before: ReturnType<typeof captureBasketMotion>) {
    if (document.hidden || typeof container.animate !== 'function') return;
    const reduced = matchMedia('(prefers-reduced-motion: reduce)');
    const animations: Animation[] = [];
    const easing = 'cubic-bezier(0.16, 1, 0.3, 1)';
    const bounds = container.getBoundingClientRect();
    for (const row of container.querySelectorAll<HTMLElement>(rows)) {
        if (before.ids.has(row.dataset.id)) continue;
        const rect = row.getBoundingClientRect();
        if (rect.bottom <= bounds.top || rect.top >= bounds.bottom) continue;
        animations.push(row.animate(reduced.matches
            ? [{ opacity: 0.65 }, { opacity: 1 }]
            : [{ opacity: 0.65, transform: 'translateX(-6px)' }, { opacity: 1, transform: 'none' }],
        { duration: reduced.matches ? 120 : 200, easing }));
    }
    // Read all final transforms before starting any writes. A rapid update starts
    // at the previous animation's displayed frame rather than its old target.
    const changes = segments.flatMap(name => {
        const element = container.querySelector<HTMLElement>(`.${name}`);
        const from = before.transforms.get(name);
        if (!element || !from || reduced.matches) return [];
        const to = getComputedStyle(element).transform;
        return from === to ? [] : [{ element, from, to }];
    });
    for (const { element, from, to } of changes) {
        animations.push(element.animate([{ transform: from }, { transform: to }], { duration: 300, easing }));
    }
    if (!animations.length) return;
    const cancel = () => animations.forEach(animation => animation.cancel());
    const onVisibility = () => { if (document.hidden) cancel(); };
    reduced.addEventListener('change', cancel, { once: true });
    document.addEventListener('visibilitychange', onVisibility);
    void Promise.allSettled(animations.map(animation => animation.finished)).then(() => {
        reduced.removeEventListener('change', cancel);
        document.removeEventListener('visibilitychange', onVisibility);
    });
}
