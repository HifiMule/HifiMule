import { t } from '../i18n';

export type TraySupport = { state: 'available' | 'missing' | 'unknown' | 'not-applicable'; fedora: boolean };
const extensionPage = 'https://extensions.gnome.org/extension/615/appindicator-support/';
const dismissalKey = 'hifimule.traySetupDismissed';
let dismissedInMemory = false;

/** A body sibling survives onboarding rerenders; its identity expires on shutdown. */
export function mountTraySetupGuidance(
    probe: () => Promise<TraySupport>,
    openPage: (url: string) => Promise<void>,
    body: HTMLElement = document.body,
    storage?: Pick<Storage, 'getItem' | 'setItem'>,
): void {
    let dismissed = dismissedInMemory;
    try { storage ??= sessionStorage; dismissed ||= storage.getItem(dismissalKey) === 'true'; } catch { /* Memory fallback. */ }
    if (dismissed) return;
    const anchor = document.createElement('aside');
    anchor.className = 'tray-setup-guidance';
    anchor.hidden = true;
    anchor.setAttribute('aria-label', t('traySetup.title'));
    body.append(anchor);
    let returnFocus: HTMLElement | null = null;
    anchor.addEventListener('focusin', event => {
        const previous = (event as FocusEvent).relatedTarget;
        if (previous instanceof HTMLElement && !anchor.contains(previous)) returnFocus = previous;
    });
    const restoreFocus = () => {
        if (!anchor.contains(document.activeElement)) return;
        if (returnFocus?.isConnected && !returnFocus.closest('[hidden], [inert]')) {
            returnFocus.focus();
            if (!anchor.contains(document.activeElement) && document.activeElement !== document.body) return;
        }
        // The root remains available when onboarding has replaced the old control.
        const root = body.querySelector<HTMLElement>('.app-container, main') ?? body;
        const previousTabIndex = root.getAttribute('tabindex');
        root.setAttribute('tabindex', '-1');
        root.focus();
        if (previousTabIndex === null) root.removeAttribute('tabindex');
        else root.setAttribute('tabindex', previousTabIndex);
    };
    let checking = false;
    let visible = false;
    let result: TraySupport | undefined;
    let status: HTMLElement | undefined;
    let checkButton: HTMLButtonElement | undefined;
    const current = () => anchor.isConnected && anchor.parentElement === body && !dismissed;
    const show = () => {
        if (!result || visible) return;
        visible = true;
        anchor.hidden = false;
        const title = document.createElement('h2'); title.textContent = t('traySetup.title');
        const explanation = document.createElement('p'); explanation.textContent = t('traySetup.body');
        const instructions = document.createElement('p'); instructions.textContent = t(result.fedora ? 'traySetup.fedora' : 'traySetup.generic');
        anchor.append(title, explanation, instructions);
        if (result.fedora) {
            const command = document.createElement('code'); command.textContent = 'sudo dnf install gnome-shell-extension-appindicator'; anchor.append(command);
        }
        const activation = document.createElement('p'); activation.textContent = t('traySetup.activation'); anchor.append(activation);
        const actions = document.createElement('div'); actions.className = 'tray-setup-actions';
        const page = document.createElement('button'); page.type = 'button'; page.textContent = t('traySetup.extensionPage');
        page.addEventListener('click', () => { void openPage(extensionPage).catch(() => { if (current() && status) status.textContent = t('traySetup.linkFailed'); }); });
        checkButton = document.createElement('button'); checkButton.type = 'button'; checkButton.textContent = t('traySetup.checkAgain');
        checkButton.addEventListener('click', () => { void check(); });
        const dismiss = document.createElement('button'); dismiss.type = 'button'; dismiss.textContent = t('traySetup.dismiss');
        dismiss.addEventListener('click', () => {
            dismissed = dismissedInMemory = true;
            try { storage?.setItem(dismissalKey, 'true'); } catch { /* Keep in memory. */ }
            restoreFocus();
            anchor.remove();
        });
        actions.append(page, checkButton, dismiss); anchor.append(actions);
        status = document.createElement('p'); status.setAttribute('role', 'status'); status.setAttribute('aria-live', 'polite'); anchor.append(status);
    };
    const check = async () => {
        if (checking || !current()) return;
        checking = true;
        if (checkButton) { checkButton.setAttribute('aria-disabled', 'true'); checkButton.setAttribute('aria-busy', 'true'); }
        try {
            const next = await probe();
            if (!current()) return;
            result = next;
            if (next.state === 'available' || next.state === 'not-applicable') { restoreFocus(); anchor.hidden = true; visible = false; anchor.replaceChildren(); }
            else if (next.state === 'missing') { show(); if (status) status.textContent = t('traySetup.stillMissing'); }
            else if (visible && status) status.textContent = t('traySetup.unknown');
        } catch {
            if (current() && visible && status) status.textContent = t('traySetup.unknown');
        } finally { checking = false; if (checkButton) { checkButton.setAttribute('aria-disabled', 'false'); checkButton.setAttribute('aria-busy', 'false'); } }
    };
    void check();
}
