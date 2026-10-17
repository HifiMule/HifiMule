// Toast notifications — lightweight wrapper around Shoelace's sl-alert.toast().
// Wait for the registered custom element before toasting on first use.

type ToastVariant = 'primary' | 'success' | 'neutral' | 'warning' | 'danger';

// Error details remain available until dismissed, including existing callers
// that explicitly request the shared error duration.
export const ERROR_TOAST_DURATION = Infinity;

const VARIANT_ICONS: Record<ToastVariant, string> = {
    primary: 'info-circle',
    success: 'check2-circle',
    neutral: 'gear',
    warning: 'exclamation-triangle',
    danger: 'exclamation-octagon',
};

function escapeHtml(text: string): string {
    const div = document.createElement('div');
    div.textContent = text;
    return div.innerHTML;
}

export function showToast(
    message: string,
    variant: ToastVariant = 'primary',
    duration = variant === 'danger' ? ERROR_TOAST_DURATION : 3000,
): void {
    const alert = document.createElement('sl-alert') as any;
    alert.variant = variant;
    alert.closable = true;
    alert.duration = duration;
    alert.innerHTML = `<sl-icon slot="icon" name="${VARIANT_ICONS[variant]}"></sl-icon>${escapeHtml(message)}`;
    document.body.appendChild(alert);
    customElements.whenDefined('sl-alert').then(() => {
        if (typeof alert.toast === 'function') {
            alert.toast();
        } else {
            alert.open = true;
        }
    });
}
