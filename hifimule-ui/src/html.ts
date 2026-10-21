export function escapeHtml(text: string): string {
    return text.replace(/&/g, '&amp;').replace(/</g, '&lt;').replace(/>/g, '&gt;');
}

export function escapeAttr(text: string): string {
    return escapeHtml(text).replace(/"/g, '&quot;').replace(/'/g, '&#39;');
}
