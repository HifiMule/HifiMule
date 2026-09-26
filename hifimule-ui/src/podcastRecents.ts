export interface RecentEpisodeIdentity { id: string }

export function acceptRecentEpisodePage<T extends RecentEpisodeIdentity>(
    request: number,
    activeRequest: number,
    inRecentTab: boolean,
    previous: T[],
    offset: number,
    limit: number,
    page: { episodes: T[]; total: number; sourceCount: number },
): { episodes: T[]; nextOffset: number; total: number } | null {
    if (request !== activeRequest || !inRecentTab) return null;
    const seen = new Set(previous.map(episode => episode.id));
    const episodes = [...previous];
    for (const episode of page.episodes) {
        if (seen.has(episode.id)) continue;
        seen.add(episode.id);
        episodes.push(episode);
    }
    return {
        episodes,
        nextOffset: offset + limit,
        total: page.sourceCount === 0 ? episodes.length : page.total,
    };
}
