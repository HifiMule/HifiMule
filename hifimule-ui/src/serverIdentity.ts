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

export function serverTypeLabel(type: string): string {
    switch (type) {
        case 'jellyfin': return 'Jellyfin';
        case 'openSubsonic': return 'OpenSubsonic';
        case 'subsonic': return 'Subsonic';
        default: return t('server.default');
    }
}

export function defaultServerIcon(type: string): string {
    switch (type) {
        case 'jellyfin': return 'collection-play';
        case 'openSubsonic':
        case 'subsonic': return 'music-note-list';
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
    const providerLabel = serverTypeLabel(server.serverType);
    const host = serverHost(server.url);
    const label = server.name?.trim() || providerLabel || server.username || host || t('server.default');
    const icon = server.icon?.trim() || defaultServerIcon(server.serverType);
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
