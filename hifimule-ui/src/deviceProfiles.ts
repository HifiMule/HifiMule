import english from 'virtual:hifimule-locale/en';
import { t } from './i18n';
import { escapeAttr } from './html';
export interface DeviceProfileSummary {
    id: string; name: string; aliases?: string[]; description?: string; ownership?: string; revision?: number;
    defaultMusicFolder?: string | null; defaultPlaylistFolder?: string | null;
    defaultAudiobookFolder?: string | null; defaultPodcastFolder?: string | null;
}
export interface DeviceProfile extends DeviceProfileSummary {
    description: string; deviceProfile: Record<string, any> | null;
    usage?: Array<{ id: string; name?: string }>; usageIncomplete?: boolean;
    [key: string]: any;
}
/** Translate only exact bundled text; custom names and clone contents remain verbatim. */
export function displayProfile<T extends DeviceProfileSummary>(profile: T): T {
    if (profile.ownership !== 'system' && profile.id !== 'passthrough') return profile;
    const id = profile.id.replace(/^system:/, '').split(':')[0];
    const nameKey = id === 'passthrough' ? 'basket.profile.no_transcoding' : `device.profile.${id}.name`;
    const descriptionKey = id === 'passthrough' ? 'basket.profile.no_transcoding_desc' : `device.profile.${id}.description`;
    const stock = english ?? {};
    return {...profile, name: profile.name === stock[nameKey] || (id === 'passthrough' && profile.name === 'No Transcoding (Download Original)') ? t(nameKey) : profile.name,
        description: profile.description === stock[descriptionKey] ? t(descriptionKey) : profile.description};
}
export const PROFILE_EDIT = '__profile_edit__';
export const PROFILE_NEW = '__profile_new__';
export function isProfileAction(value: string): boolean { return value === PROFILE_EDIT || value === PROFILE_NEW; }
export function canonicalProfileId(profiles: DeviceProfileSummary[], id: string): string {
    return profiles.find(profile => profile.id === id || profile.aliases?.includes(id))?.id ?? id;
}
export function profileOptions(profiles: DeviceProfileSummary[], actions = true): string {
    return ['custom', 'system'].map(group => {
        const entries = profiles.filter(p => (p.ownership === 'system' ? 'system' : 'custom') === group);
        return entries.length ? `<small class="profile-option-group">${t(`profiles.${group}`)}</small>${entries.map(displayProfile).map(p => `<sl-option value="${escapeAttr(p.id)}">${escapeAttr(p.name)}</sl-option>`).join('')}` : '';
    }).join('') + (actions ? `<sl-divider></sl-divider><sl-option value="${PROFILE_EDIT}">${t('profiles.edit_selected')}</sl-option><sl-option value="${PROFILE_NEW}">${t('profiles.new')}</sl-option>` : '');
}
export const formats = [
    ['mp3', 'MP3', 'mp3', 'mp3'], ['aac', 'AAC / M4A', 'mp4', 'aac'],
    ['alac', 'ALAC / M4A', 'm4a', 'alac'], ['m4b', 'AAC / M4B', 'm4b', 'aac'],
    ['flac', 'FLAC', 'flac', 'flac'], ['vorbis', 'Ogg / Vorbis', 'ogg', 'vorbis'],
    ['opus', 'Opus', 'opus', 'opus'], ['wav', 'WAV', 'wav', 'pcm_s16le'],
] as const;
export const outputs = formats.filter(f => f[0] !== 'm4b');
export const limits = ['MaxAudioSampleRate', 'MaxAudioBitDepth', 'MaxAudioChannels', 'MaxStreamingBitrate'] as const;
export type ProfileDraft = { name: string; description: string; music: string; playlists: string; audiobooks: string; podcasts: string; mode: string; formats: string[]; output: string; bitrate: string; limits: Record<string, string> };
const copy = <T>(value: T): T => JSON.parse(JSON.stringify(value));
function rowFormats(row: any): string[] {
    if (!row || typeof row !== 'object' || row.Type !== 'Audio'
        || Object.keys(row).some(key => !['Container','AudioCodec','Type'].includes(key))
        || typeof row.Container !== 'string' || typeof row.AudioCodec !== 'string') return [];
    const containers = row.Container.toLowerCase().split(',').map((value: string) => value.trim());
    const codecs = row.AudioCodec.toLowerCase().split(',').map((value: string) => value.trim());
    const matches = (container: string, codec: string) => formats.filter(([, , knownContainer, knownCodec]) =>
        codec === knownCodec && (container === knownContainer || (['mp4','m4a'].includes(knownContainer) && ['mp4','m4a'].includes(container))));
    // A partially understood row cannot be safely reconstructed by the checkboxes.
    if (containers.some((container: string) => codecs.some((codec: string) => !matches(container,codec).length))) return [];
    return [...new Set<string>(containers.flatMap((container: string) => codecs.flatMap((codec: string) => matches(container,codec).map(format => format[0]))))];
}
export function kbpsToBps(value: string): number | undefined {
    if (!/^\d+(?:\.\d{1,3})?$/.test(value)) return undefined;
    const [whole, fraction = ''] = value.split('.');
    const bits = Number(whole) * 1000 + Number(fraction.padEnd(3,'0'));
    return Number.isSafeInteger(bits) && bits > 0 ? bits : undefined;
}
export function toDraft(profile: DeviceProfile): ProfileDraft {
    const audio = profile.deviceProfile;
    const target = audio?.TranscodingProfiles?.[0];
    const output = formats.find(([, , container, codec]) => (target?.Container === container || (container === 'mp4' && target?.Container === 'm4a')) && target?.AudioCodec === codec)?.[0] ?? 'mp3';
    return { name: profile.name, description: profile.description, music: profile.defaultMusicFolder ?? '', playlists: profile.defaultPlaylistFolder ?? '',
        audiobooks: profile.defaultAudiobookFolder ?? '', podcasts: profile.defaultPodcastFolder ?? '',
        mode: !audio ? 'originals' : audio.DirectPlayProfiles?.length ? 'compatible' : 'always',
        formats: formats.filter(([id]) => (audio?.DirectPlayProfiles?.flatMap(rowFormats) ?? []).includes(id)).map(([id]) => id), output,
        bitrate: String((audio?.MusicStreamingTranscodingBitrate ?? 256000) / 1000),
        limits: Object.fromEntries(limits.map(key => [key, audio?.[key] === undefined ? '' : String(key === 'MaxStreamingBitrate' ? audio[key] / 1000 : audio[key])])) };
}
export function unsupportedRules(profile: DeviceProfile): string[] {
    const audio = profile.deviceProfile;
    if (!audio) return [];
    const known = ['Name','DirectPlayProfiles','TranscodingProfiles','CodecProfiles','MusicStreamingTranscodingBitrate',...limits];
    const unsupported: string[] = Object.keys(audio).filter(key => !known.includes(key));
    if (audio.CodecProfiles?.some((p: any) => p.Conditions?.length)) unsupported.push('CodecProfiles');
    if (audio.DirectPlayProfiles?.some((p: any) => !rowFormats(p).length || Object.keys(p).some(k => !['Container','AudioCodec','Type'].includes(k)))) unsupported.push('DirectPlayProfiles');
    if (audio.TranscodingProfiles?.some((p: any) => Object.keys(p).some(key => !['Container','AudioCodec','Type','Protocol','EstimateContentLength','EnableMpegtsM2TsMode'].includes(key)))) unsupported.push('TranscodingProfiles rules');
    if (audio.TranscodingProfiles?.length !== 1 || !outputs.some(([, , c, codec]) => [c, c === 'mp4' ? 'm4a' : c].includes(audio.TranscodingProfiles?.[0]?.Container) && codec === audio.TranscodingProfiles?.[0]?.AudioCodec)) unsupported.push('TranscodingProfiles');
    return unsupported;
}
export function fromDraft(base: DeviceProfile, draft: ProfileDraft): DeviceProfile {
    const result = copy(base), original = toDraft(base);
    delete result.usage; delete result.usageIncomplete;
    result.name = draft.name.trim(); result.description = draft.description;
    result.defaultMusicFolder = draft.music.trim() || null; result.defaultPlaylistFolder = draft.playlists.trim() || null;
    result.defaultAudiobookFolder = draft.audiobooks.trim() || null; result.defaultPodcastFolder = draft.podcasts.trim() || null;
    // Unsupported rules remain intact and visible; only identity and folders are changed.
    const audioFields = (value: ProfileDraft) => JSON.stringify([value.mode,value.formats,value.output,value.bitrate,value.limits]);
    if (unsupportedRules(base).length || audioFields(draft) === audioFields(original)) return result;
    if (draft.mode === 'originals') { result.deviceProfile = null; return result; }
    const audio = result.deviceProfile ?? { Name: 'HifiMule-Custom', CodecProfiles: [] };
    if (!base.deviceProfile || draft.mode !== original.mode || JSON.stringify(draft.formats) !== JSON.stringify(original.formats)) {
        audio.DirectPlayProfiles = draft.mode === 'always' ? [] : formats.filter(f => draft.formats.includes(f[0])).map(([, , Container, AudioCodec]) => ({Container, AudioCodec, Type:'Audio'}));
    }
    if (!base.deviceProfile || draft.output !== original.output) {
        const [, , Container, AudioCodec] = outputs.find(f => f[0] === draft.output) ?? outputs[0];
        audio.TranscodingProfiles = [{Container, AudioCodec, Type:'Audio', Protocol:'http', EstimateContentLength:true, EnableMpegtsM2TsMode:false}];
    }
    if (!['flac','alac','wav'].includes(draft.output)) audio.MusicStreamingTranscodingBitrate = kbpsToBps(draft.bitrate);
    else delete audio.MusicStreamingTranscodingBitrate;
    for (const key of limits) {
        if (draft.limits[key] === '') delete audio[key];
        else audio[key] = key === 'MaxStreamingBitrate' ? kbpsToBps(draft.limits[key]) : Number(draft.limits[key]);
    }
    result.deviceProfile = audio; return result;
}
export function validateDraft(draft: ProfileDraft): string | undefined {
    if (!draft.name.trim() || draft.name.trim().length > 120) return t('profiles.invalid_name');
    for (const folder of [draft.music,draft.playlists,draft.audiobooks,draft.podcasts]) if (/^[\/]|[\\:\0]/.test(folder) || folder.split('/').some(s => s === '.' || s === '..')) return t('profiles.invalid_folder');
    for (const [key,value] of Object.entries(draft.limits)) if (value !== '' && (key === 'MaxStreamingBitrate' ? kbpsToBps(value) === undefined : !Number.isInteger(Number(value)) || Number(value) <= 0)) return t('profiles.invalid_limit');
    if (draft.mode !== 'originals' && !['flac','alac','wav'].includes(draft.output) && kbpsToBps(draft.bitrate) === undefined) return t('profiles.invalid_limit');
    return undefined;
}
export function newProfile(): DeviceProfile {
    return {id:'',name:t('profiles.untitled'),description:'',ownership:'custom',revision:0,deviceProfile:{Name:'HifiMule-Custom',DirectPlayProfiles:[{Container:'mp3',AudioCodec:'mp3',Type:'Audio'}],TranscodingProfiles:[{Container:'mp3',AudioCodec:'mp3',Type:'Audio',Protocol:'http',EstimateContentLength:true,EnableMpegtsM2TsMode:false}],CodecProfiles:[],MusicStreamingTranscodingBitrate:256000}};
}

/** Action values are restored synchronously before another listener or a submit can observe them. */
export function bindProfileActions(select: any, initial: string, profiles: DeviceProfileSummary[], changed: (profiles: DeviceProfileSummary[], selected: DeviceProfile) => void): void {
    let selected = initial;
    select.addEventListener('sl-change', (event: Event) => {
        if (event.target !== select) return;
        const action = select.value;
        if (!isProfileAction(action)) { selected = action; return; }
        event.stopImmediatePropagation(); select.value = selected;
        void import('./components/DeviceProfileManager').then(({openDeviceProfiles}) => openDeviceProfiles({
            selectedId:selected,create:action === PROFILE_NEW,returnFocus:select,
            onSaved:async profile => {
                const {rpcCall} = await import('./rpc');
                const refreshed = await rpcCall('device_profiles.list');
                profiles.splice(0,profiles.length,...refreshed);
                select.innerHTML = profileOptions(profiles); selected = profile.id; select.value = selected;
                changed(profiles,profile);
            },
        }));
    }, true);
}
