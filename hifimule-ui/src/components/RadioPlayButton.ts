import { playbackGetSelectionConfig, playbackSaveSelectionConfig, playbackStartSelection } from '../rpc';
import type { BrowseDisplayItem } from './MediaCard';
import { t } from '../i18n';
import { showToast } from '../toast';

// Shared across cards and rows: source replacement and startup form one operation.
let starting = false;

export function createRadioPlayButton(item: BrowseDisplayItem): HTMLElement | null {
    const kind = item.type === 'MusicArtist' && item.basketType !== 'BookAuthor'
        ? 'artist' : item.type === 'Playlist' && item.playablePlaylist ? 'playlist' : null;
    if (!kind || !item.serverId || !item.id || (kind === 'playlist' && item.childCount === 0)) return null;
    const source = { serverId: item.serverId, kind, ref: item.id } as const;
    const button = document.createElement('sl-icon-button') as HTMLElement & { name: string; label: string; disabled: boolean };
    button.name = 'broadcast';
    button.label = t(`playback.radio.start_${kind}`, { title: item.name });
    button.setAttribute('aria-label', button.label);
    const tooltip = document.createElement('sl-tooltip');
    tooltip.setAttribute('content', button.label);
    tooltip.setAttribute('hoist', '');
    tooltip.appendChild(button);
    button.addEventListener('mousedown', event => event.stopPropagation());
    button.addEventListener('click', async event => {
        event.stopPropagation();
        if (starting || button.disabled) return;
        starting = true;
        button.disabled = true;
        let stage: 'read' | 'save' | 'start' = 'read';
        try {
            const config = await playbackGetSelectionConfig();
            stage = 'save';
            await playbackSaveSelectionConfig({ ...config, sources: [source] });
            stage = 'start';
            await playbackStartSelection();
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
                : code.includes('PLAYBACK_SELECTION_SETUP') ? 'playback.selection.invalid'
                : 'playback.selection.unavailable';
            showToast(t(key), 'danger');
        } finally {
            starting = false;
            button.disabled = false;
        }
    });
    return tooltip;
}
