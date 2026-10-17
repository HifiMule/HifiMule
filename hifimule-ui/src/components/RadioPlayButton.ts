import { playbackGetSelectionConfig, playbackSaveSelectionConfig, playbackStartSelection } from '../rpc';
import type { BrowseDisplayItem } from './MediaCard';
import { t } from '../i18n';
import { showToast } from '../toast';

// Shared across cards and rows: source replacement and startup form one operation.
let starting = false;
const mountedActions = new Set<RadioAction>();

// Subscribe only while mounted: browsing and virtual rows routinely discard controls.
class RadioAction extends HTMLElement {
    refresh?: () => void;
    connectedCallback(): void { mountedActions.add(this); this.refresh?.(); }
    disconnectedCallback(): void { mountedActions.delete(this); }
}
if (!customElements.get('hm-radio-action')) customElements.define('hm-radio-action', RadioAction);

function refreshActions(): void {
    for (const action of mountedActions) action.refresh?.();
}

export function createRadioPlayButton(item: BrowseDisplayItem): HTMLElement | null {
    const kind = item.type === 'MusicArtist' && item.basketType !== 'BookAuthor'
        ? 'artist' : item.type === 'Playlist' && item.playablePlaylist ? 'playlist'
        : item.type === 'MusicGenre' ? 'genre' : null;
    if (!kind || !item.serverId || !item.id || (kind === 'playlist' && item.childCount === 0)) return null;
    const source = { serverId: item.serverId, kind, ref: item.id } as const;
    const button = document.createElement('sl-icon-button') as HTMLElement & { name: string; label: string; disabled: boolean; updateComplete: Promise<boolean> };
    button.name = 'broadcast';
    button.label = t(`playback.radio.start_${kind}`, { title: item.name });
    button.setAttribute('aria-label', button.label);
    const tooltip = document.createElement('sl-tooltip');
    tooltip.setAttribute('content', button.label);
    tooltip.setAttribute('hoist', '');
    const action = document.createElement('hm-radio-action') as RadioAction;
    action.appendChild(button);
    tooltip.appendChild(action);
    action.refresh = () => {
        const label = starting ? t('playback.selection.starting') : t(`playback.radio.start_${kind}`, { title: item.name });
        button.name = starting ? 'hourglass-split' : 'broadcast';
        button.label = label;
        button.setAttribute('aria-label', label);
        button.setAttribute('aria-disabled', String(starting));
        if (starting) button.setAttribute('aria-busy', 'true');
        else button.removeAttribute('aria-busy');
        tooltip.setAttribute('content', label);
        // Shoelace's focusable button lives in shadow DOM; host ARIA alone is
        // not exposed consistently by accessibility trees. Keep it focusable.
        void button.updateComplete?.then(() => {
            const control = button.shadowRoot?.querySelector('button');
            control?.setAttribute('aria-disabled', String(starting));
            control?.setAttribute('aria-busy', String(starting));
        });
    };
    action.refresh?.();
    button.addEventListener('mousedown', event => event.stopPropagation());
    button.addEventListener('click', async event => {
        event.stopPropagation();
        if (starting || button.disabled) return;
        starting = true;
        refreshActions();
        action.refresh?.();
        showToast(t('playback.selection.starting'));
        let stage: 'read' | 'save' | 'start' = 'read';
        try {
            const config = await playbackGetSelectionConfig();
            stage = 'save';
            await playbackSaveSelectionConfig({ ...config, sources: [source] });
            stage = 'start';
            await playbackStartSelection();
            showToast(t('playback.selection.started'), 'success');
        } catch (error) {
            const code = (error as { data?: { code?: string } })?.data?.code ?? String(error);
            const key = stage === 'read' ? 'playback.selection.load_failed'
                : stage === 'save' ? 'playback.selection.save_failed'
                : code.includes('OUTPUT_') ? 'playback.output.choose'
                : code.includes('PLAYBACK_SELECTION_CANCELLED') ? 'playback.selection.cancelled'
                : code.includes('PLAYBACK_BUSY') ? 'playback.selection.busy'
                : code.includes('PERSISTENCE_FAILED') ? 'playback.command_error.persistence'
                : code.includes('PLAYBACK_SELECTION_PREPARATION_FAILED') ? 'playback.selection.preparation_failed'
                : code.includes('PLAYBACK_SELECTION_EMPTY') ? 'playback.selection.empty'
                : code.includes('PLAYBACK_SELECTION_NO_MUSIC_SERVER') ? 'playback.selection.no_music_server'
                    : code.includes('PLAYBACK_SELECTION_SETUP') ? 'playback.selection.invalid'
                : 'playback.selection.unavailable';
            showToast(t(key), 'danger');
        } finally {
            starting = false;
            refreshActions();
            action.refresh?.();
        }
    });
    return tooltip;
}
