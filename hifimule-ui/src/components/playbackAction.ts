import { showToast } from '../toast';

export function playbackAction({ icon, label, run }: {
    icon: string;
    label: string;
    run: (() => Promise<unknown>) | null;
}): HTMLElement & { name: string; label: string; disabled: boolean } {
    const button = document.createElement('sl-icon-button') as HTMLElement & { name: string; label: string; disabled: boolean };
    button.name = icon;
    button.label = label;
    button.disabled = !run;
    button.addEventListener('mousedown', event => event.stopPropagation());
    button.addEventListener('click', async event => {
        event.stopPropagation();
        if (!run || button.disabled) return;
        button.disabled = true;
        button.setAttribute('aria-busy', 'true');
        button.name = 'hourglass-split';
        try { await run(); }
        catch (error) { showToast((error as Error).message, 'danger'); }
        finally {
            button.disabled = false;
            button.removeAttribute('aria-busy');
            button.name = icon;
        }
    });
    return button;
}
