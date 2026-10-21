import { t } from './i18n';

export function deviceIconLabel(icon: string): string {
    const labels: Record<string, string> = {
        'usb-drive': t('basket.icon.usb_drive'),
        'phone-fill': t('basket.icon.phone'),
        'watch': t('basket.icon.watch'),
        'sd-card': t('basket.icon.sd_card'),
        'headphones': t('basket.icon.headphones'),
        'music-note-list': t('basket.icon.music_player'),
    };
    return labels[icon] ?? icon;
}
