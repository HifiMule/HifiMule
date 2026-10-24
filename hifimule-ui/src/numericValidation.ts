import { t } from './i18n';

export interface NumericRules {
    required?: boolean; integer?: boolean; min?: number; max?: number;
    decimals?: number; scale?: number; float32?: boolean;
}
/** Validate the raw draft before Number() can turn an empty field into zero. */
export function numericError(raw: string, rules: NumericRules = {}): string | undefined {
    const value = raw.trim();
    if (!value) return rules.required ? t('numeric.required') : undefined;
    if (!/^[+-]?(?:\d+(?:\.\d*)?|\.\d+)(?:e[+-]?\d+)?$/i.test(value) || !Number.isFinite(Number(value))) return t('numeric.finite');
    const n = Number(value);
    if (n === 0 && /[1-9]/.test(value.split(/[eE]/)[0])) return t('numeric.converted');
    if (rules.integer) {
        const [mantissa, exponent = '0'] = value.toLowerCase().split('e');
        const places = (mantissa.split('.')[1]?.length ?? 0) - Number(exponent);
        if (!Number.isSafeInteger(n) || (places > 0 && /[1-9]/.test(mantissa.replace('.', '').slice(-places)))) return t('numeric.integer');
    }
    if (rules.decimals !== undefined && !new RegExp(`^\\d+(?:\\.\\d{1,${rules.decimals}})?$`).test(value)) return t('numeric.precision', { count: rules.decimals });
    if (rules.min !== undefined && n < rules.min) return t('numeric.minimum', { min: rules.min });
    if (rules.max !== undefined && n > rules.max) return t('numeric.maximum', { max: rules.max });
    if (rules.float32 && !Number.isFinite(Math.fround(n))) return t('numeric.finite');
    if (rules.float32 && n !== 0 && Math.fround(n) === 0) return t('numeric.converted');
    if (rules.scale !== undefined && (!Number.isSafeInteger(Math.round(n * rules.scale)) || (n > 0 && Math.round(n * rules.scale) === 0))) return t('numeric.converted');
    return undefined;
}
export function monthListError(raw: string): string | undefined {
    return raw.trim() && raw.split(',').some(value => !value.trim() || numericError(value, { required: true, integer: true, min: 1, max: 12 })) ? t('numeric.months') : undefined;
}
let nextFeedback = 0;
/** Use the component's validity API and a stable, described inline message for both control types. */
export function fieldFeedback(control: any, error?: string, hint = ''): void {
    if (!control?.setAttribute) return;
    control._numericValidity = error ?? '';
    const syncValidity = () => control.setCustomValidity?.(control._numericValidity);
    // Shoelace's input is not available until its first Lit render completes.
    if (control.updateComplete) void control.updateComplete.then(syncValidity);
    else syncValidity();
    control.setAttribute('aria-invalid', String(!!error));
    let feedback = control._numericFeedback as HTMLElement | undefined;
    if (!feedback) {
        feedback = document.createElement('div'); feedback.id = `numeric-feedback-${++nextFeedback}`;
        feedback.className = 'numeric-feedback'; feedback.setAttribute('aria-live', 'polite');
        if (control.tagName?.startsWith('SL-')) { feedback.slot = 'help-text'; control.append(feedback); }
        else control.insertAdjacentElement('afterend', feedback);
        control.setAttribute('aria-describedby', [control.getAttribute('aria-describedby'), feedback.id].filter(Boolean).join(' '));
        control._numericFeedback = feedback;
    }
    feedback.textContent = error ?? hint; feedback.classList.toggle('numeric-feedback--error', !!error);
}
export function revealInvalid(control: any): void {
    if (!control) return;
    let ancestor = control.parentElement;
    while (ancestor) { if (ancestor.matches?.('details, sl-details')) ancestor.open = true; ancestor = ancestor.parentElement; }
    const focus = () => { control.scrollIntoView?.({ block: 'nearest' }); control.focus?.(); };
    if (control.updateComplete) void control.updateComplete.then(focus);
    else focus();
}
