import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import test from 'node:test';
import ts from 'typescript';

const source = readFileSync(new URL('../src/components/BasketSidebar.ts', import.meta.url), 'utf8')
    .replace(/^import .*;\r?\n/gm, '').replace(/export class /g, 'class ');
const script = ts.transpile(source, { target: ts.ScriptTarget.ES2022 });
const catalogs = JSON.parse(readFileSync(new URL('../../hifimule-i18n/catalog.json', import.meta.url), 'utf8'));
const now = Date.parse('2026-10-23T12:00:00Z');
class ClockDate extends Date { static now() { return now; } }

test('sync ETA switches to localized hours and minutes at one hour and carries rounded minutes', () => {
    for (const [locale, expected] of Object.entries({
        en: ['~1 h left', '~1 h 30 min left', '~2 h left', '~25 h 1 min left'],
        fr: ['Encore ~1 h', '~1 h 30 min restantes', 'Encore ~2 h', '~25 h 1 min restantes'],
        es: ['~1 h para terminar', '~1 h 30 min restantes', '~2 h para terminar', '~25 h 1 min restantes'],
        de: ['~1 Std. verbleibend', '~1 Std. 30 Min. verbleibend', '~2 Std. verbleibend', '~25 Std. 1 Min. verbleibend'],
    })) {
        const t = (key, values = {}) => Object.entries(values).reduce(
            (text, [name, value]) => text.replaceAll(`{${name}}`, String(value)), catalogs[locale][key],
        );
        const BasketSidebar = new Function('t', 'Date', `${script}\nreturn BasketSidebar;`)(t, ClockDate);
        const sidebar = Object.create(BasketSidebar.prototype);
        // One byte per second makes the remaining byte count equal the ETA in seconds.
        const operation = { startedAt: new Date(now - 1000).toISOString(), bytesTransferred: 1 };
        const eta = seconds => sidebar.computeEta({ ...operation, totalBytes: seconds + 1 });

        assert.equal(eta(9), t('basket.sync.almost_done'));
        assert.equal(eta(42), t('basket.sync.seconds_left', { count: 42 }));
        assert.equal(eta(60), t('basket.sync.minutes_left', { count: 1 }));
        assert.equal(eta(3540), t('basket.sync.minutes_left', { count: 59 }));
        assert.equal(eta(3599), t('basket.sync.minutes_left', { count: 60 }));
        for (const [index, seconds] of [3600, 5400, 7199, 90060].entries()) {
            assert.equal(eta(seconds), expected[index], `${locale}: ${seconds}s`);
        }
        assert.equal(eta(7200), expected[2], `${locale}: exactly two hours`);
        assert.equal(sidebar.computeEta({ ...operation, totalBytes: 0 }), t('basket.sync.calculating'));
    }
});
