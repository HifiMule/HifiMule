// Toast notifications — lightweight wrapper around Shoelace's sl-alert.toast().
// Shoelace components auto-register via the CDN autoloader; we wait for the
// custom element before toasting so the call doesn't no-op on first use.

type ToastVariant = 'primary' | 'success' | 'neutral' | 'warning' | 'danger';

// Error toasts often carry a raw backend {message}; give them longer than the
// 3s default so the detail can actually be read before it auto-dismisses.
export const ERROR_TOAST_DURATION = 6000;

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
    duration = 3000,
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
