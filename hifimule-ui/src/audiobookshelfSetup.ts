import type { AudiobookshelfLibraryChoice } from './rpc';

export type LoginProviderChoice = 'auto' | 'jellyfin' | 'subsonic' | 'audiobookshelf';

export function isLoginProviderChoice(value: string): value is LoginProviderChoice {
    return value === 'auto' || value === 'jellyfin' || value === 'subsonic' || value === 'audiobookshelf';
}

export function shouldUseAudiobookshelfDiscovery(
    selectedProvider: LoginProviderChoice,
    detectedProvider: string | null,
): boolean {
    return selectedProvider === 'audiobookshelf'
        || (selectedProvider === 'auto' && detectedProvider === 'audiobookshelf');
}

export function audiobookshelfRoleLabelKey(role: AudiobookshelfLibraryChoice['role']):
    'login.audiobookshelf.role_books' | 'login.audiobookshelf.role_podcasts' {
    return role === 'audiobook'
        ? 'login.audiobookshelf.role_books'
        : 'login.audiobookshelf.role_podcasts';
}

export function validLibraryChoices(value: unknown): AudiobookshelfLibraryChoice[] {
    if (!Array.isArray(value)) return [];
    return value.filter((choice): choice is AudiobookshelfLibraryChoice => {
        if (!choice || typeof choice !== 'object') return false;
        const item = choice as Record<string, unknown>;
        return typeof item.choiceId === 'string'
            && item.choiceId.length > 0
            && typeof item.name === 'string'
            && item.name.trim().length > 0
            && (item.role === 'audiobook' || item.role === 'podcast');
    });
}
