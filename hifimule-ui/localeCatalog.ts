import { readFileSync } from 'node:fs';
import type { Plugin } from 'vite';

/** Split the shared daemon/UI catalog at build time, without generated copies. */
export function localeCatalog(path: string): Plugin {
    const prefix = 'virtual:hifimule-locale/';
    return {
        name: 'hifimule-locale-catalog',
        resolveId(id) {
            if (id.startsWith(prefix) && /^(en|fr|es|de)$/.test(id.slice(prefix.length))) {
                return '\0' + id;
            }
        },
        load(id) {
            if (!id.startsWith('\0' + prefix)) return;
            this.addWatchFile(path);
            const language = id.slice(prefix.length + 1);
            const catalog = JSON.parse(readFileSync(path, 'utf8'));
            if (!catalog[language]) throw new Error(`Missing translation catalog: ${language}`);
            return `export default ${JSON.stringify(catalog[language])};`;
        },
        handleHotUpdate({ file, server }) {
            if (file !== path) return;
            for (const module of server.moduleGraph.getModulesByFile(path) ?? []) {
                server.moduleGraph.invalidateModule(module);
            }
            for (const language of ['en', 'fr', 'es', 'de']) {
                const module = server.moduleGraph.getModuleById('\0' + prefix + language);
                if (module) server.moduleGraph.invalidateModule(module);
            }
            server.ws.send({ type: 'full-reload' });
            return [];
        },
    };
}
