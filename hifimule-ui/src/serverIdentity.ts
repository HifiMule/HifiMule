import type { ServerSummary } from './rpc';
import { t } from './i18n';

export const SERVER_ICON_OPTIONS = [
    'hdd-network',
    'server',
    'music-note-list',
    'music-note-beamed',
    'headphones',
    'collection-play',
    'disc',
    'broadcast-pin',
    'book',
] as const;

export function serverTypeLabel(type: string, role?: ServerSummary['libraryRole']): string {
    switch (type) {
        case 'jellyfin': return 'Jellyfin';
        case 'openSubsonic': return 'OpenSubsonic';
        case 'subsonic': return 'Subsonic';
        case 'audiobookshelf':
            if (role === 'audiobook') return t('server.audiobookshelf.books');
            if (role === 'podcast') return t('server.audiobookshelf.podcasts');
            return 'Audiobookshelf';
        default: return t('server.default');
    }
}

export function defaultServerIcon(type: string, role?: ServerSummary['libraryRole']): string {
    switch (type) {
        case 'jellyfin': return 'collection-play';
        case 'openSubsonic':
        case 'subsonic': return 'music-note-list';
        case 'audiobookshelf': return role === 'podcast' ? 'broadcast-pin' : 'book';
        default: return 'hdd-network';
    }
}

export interface ServerIdentity {
    label: string;
    icon: string;
    providerLabel: string;
    host: string;
    secondaryText: string;
    tooltip: string;
}

export function serverHost(url: string): string {
    try {
        return new URL(url).host;
    } catch {
        return url.replace(/^https?:\/\//i, '').replace(/\/+$/, '') || url;
    }
}

export function formatServerIdentity(server: ServerSummary): ServerIdentity {
    const providerLabel = serverTypeLabel(server.serverType, server.libraryRole);
    const host = serverHost(server.url);
    const label = server.name?.trim() || providerLabel || server.username || host || t('server.default');
    const icon = server.icon?.trim() || defaultServerIcon(server.serverType, server.libraryRole);
    const secondaryParts = [providerLabel, server.username, host].filter(Boolean);
    const secondaryText = secondaryParts.join(' - ');
    return {
        label,
        icon,
        providerLabel,
        host,
        secondaryText,
        tooltip: `${label} - ${secondaryText}`,
    };
}
