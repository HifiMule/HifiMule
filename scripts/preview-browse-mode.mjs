// Reproducible browser fixture: production modules + Shoelace, mocked Tauri IPC.
// Run: rtk node scripts/preview-browse-mode.mjs
// Open http://localhost:1422/.browse-preview.html. Ctrl-C removes generated files.
import { readFileSync, writeFileSync, unlinkSync } from 'node:fs';
import { execFileSync } from 'node:child_process';
import { createServer } from '../hifimule-ui/node_modules/vite/dist/node/index.js';
const baseline = '2a88f9b1b410e14ef8fe22e80fcff8aef1d4a3de';
const files = [];
function write(path, text) { writeFileSync(path, text, {flag:'wx'}); files.push(path); }
const probe = '\nexport const browseProbe = { state, renderModeBar, switchMode };';
write('hifimule-ui/src/.browse-current.ts', readFileSync('hifimule-ui/src/library.ts','utf8') + probe);
write('hifimule-ui/src/.browse-before.ts', execFileSync('rtk',['proxy','git','show',baseline+':hifimule-ui/src/library.ts'],{encoding:'utf8'}) + probe);
write('hifimule-ui/.browse-before.css', execFileSync('rtk',['proxy','git','show',baseline+':hifimule-ui/src/styles.css'],{encoding:'utf8'}));
write('hifimule-ui/.browse-preview.html', `<!doctype html><html class="sl-theme-dark"><meta charset="utf-8"><title>15.16 production renderer verification</title>
<style>.fixture-controls{box-sizing:border-box;height:64px;display:flex;align-items:center;gap:12px;padding:8px;font:12px system-ui;flex-wrap:wrap}.fixture-controls select{color:inherit;background:#151d31}.fixture-controls label{display:flex;gap:4px}.fixture-shell .basket-view{height:100%;min-height:0}.fixture-shell{height:calc(100vh - 64px);width:720px;max-width:100%;resize:horizontal;overflow:auto;min-width:240px}.fixture-status{margin:0;padding:8px;font:12px system-ui;white-space:pre-wrap;overflow:auto}.library-view{height:100%}#playback-controls-container{flex-shrink:0;min-height:64px;padding:8px;background:var(--panel-bg);font-size:12px}</style>
<div class="fixture-controls"><label>Version<select id="version"><option value="current">Current</option><option value="before">Before</option></select></label><label>Width<select id="width"><option>720</option><option>360</option><option>1000</option></select></label><label>Language<select id="lang"><option>en</option><option>fr</option><option>es</option><option>de</option></select></label><label>Text<select id="scale"><option value="1">100%</option><option value="2">200%</option></select></label><label>Modes<select id="modes"><option>Broad</option><option>Limited</option><option>Empty</option></select></label><button id="refresh">Refresh</button><button id="loading">Loading</button><button id="surface">Library / Playing</button><button id="split">Divider layout</button></div>
<div class="fixture-shell"><main class="library-view"><header><div class="library-header-row"><div class="library-title-block"><h1>Library</h1><p>Fixture library</p></div></div></header><div id="browse-mode-bar"></div><div id="library-content"></div><div id="playback-destination-container" hidden>Playing (fixture)</div><div id="playback-controls-container">Playback region (fixture — no audio)<pre class="fixture-status" id="metrics"></pre></div></main></div>
<script type="module">
import '@shoelace-style/shoelace/dist/themes/dark.css';
import '@shoelace-style/shoelace/dist/shoelace.js';
import {setBasePath} from '@shoelace-style/shoelace/dist/utilities/base-path.js';
import {mockIPC,mockWindows} from '@tauri-apps/api/mocks';
setBasePath('/node_modules/@shoelace-style/shoelace/dist');mockWindows('main');
const broad=['artists','albums','playlists','genres','tracks','recentlyAdded','frequentlyPlayed','recentlyPlayed','favorites'];
const calls=[];let available=broad;
mockIPC(async(cmd,payload)=>{if(cmd!=='rpc_proxy')return null;const m=payload.method;calls.push(m);document.querySelector('#metrics').dataset.calls=JSON.stringify(calls);
if(m==='browse.listModes')return {modes:available};
if(m==='get_daemon_state')return {basketItems:[{id:'kept',name:'Fixture basket item',type:'Audio',childCount:1,sizeTicks:0,sizeBytes:0}],destinations:[]};
if(m.startsWith('browse.'))return {artists:[],albums:[],tracks:[],genres:[],playlists:[],total:0};
if(m==='playback.getSession')return {data:null};
throw Error('Unexpected mutation/request: '+m);},{shouldMockEvents:true});
const q=new URLSearchParams(location.search);const before=q.get('version')==='before';document.querySelector('#version').value=before?'before':'current';
const css=document.createElement('link');css.rel='stylesheet';css.href=before?'/.browse-before.css':'/src/styles.css';document.head.append(css);
const {setLanguage}=await import('/src/i18n.ts');setLanguage(q.get('lang')||'en');
const {initLibraryView,browseProbe}=await import(before?'/src/.browse-before.ts':'/src/.browse-current.ts');
const {state,renderModeBar}=browseProbe;
for(const key of ['width','lang','scale'])if(q.has(key))document.querySelector('#'+key).value=q.get(key);
function measure(){requestAnimationFrame(()=>{const b=document.querySelector('#browse-mode-bar'),c=document.querySelector('#library-content');const buttons=[...b.querySelectorAll('[data-mode]')];document.querySelector('#metrics').textContent=JSON.stringify({width:b.clientWidth,barHeight:b.offsetHeight,contentHeight:c.clientHeight,rows:new Set(buttons.map(n=>n.offsetTop)).size,gap:getComputedStyle(b.querySelector('.browse-mode-bar')).gap,buttonHeight:buttons[0]?.offsetHeight,font:buttons[0]?getComputedStyle(buttons[0].shadowRoot.querySelector('button')).fontSize:null,overflow:b.scrollWidth>b.clientWidth,mode:state.browseMode},null,0)})}
function controls(){document.querySelector('.fixture-shell').style.width=document.querySelector('#width').value+'px';document.documentElement.style.fontSize=(16*Number(document.querySelector('#scale').value))+'px';setLanguage(document.querySelector('#lang').value);renderModeBar();measure()}
for(const id of ['width','lang','scale'])document.querySelector('#'+id).onchange=controls;
document.querySelector('#version').onchange=e=>{const params=new URLSearchParams();for(const id of ['version','width','lang','scale'])params.set(id,document.querySelector('#'+id).value);location.search=params.toString()};
document.querySelector('#modes').onchange=async e=>{available=e.target.value==='Broad'?broad:e.target.value==='Limited'?['albums','tracks','favorites']:[];await initLibraryView();measure()};
document.querySelector('#refresh').onclick=()=>{renderModeBar();measure()};
document.querySelector('#loading').onclick=()=>{state.loading=!state.loading;renderModeBar();measure()};
document.querySelector('#surface').onclick=()=>{const b=document.querySelector('#browse-mode-bar'),c=document.querySelector('#library-content'),p=document.querySelector('#playback-destination-container');b.hidden=c.hidden=!b.hidden;p.hidden=!b.hidden};
document.querySelector('#split').onclick=()=>{const shell=document.querySelector('.fixture-shell');if(shell.querySelector('sl-split-panel'))return;shell.style.width='100%';const panel=document.createElement('sl-split-panel');panel.className='split-panel';panel.primary='end';panel.position=32;const library=shell.querySelector('.library-view');library.slot='start';const basket=document.createElement('aside');basket.slot='end';basket.className='basket-view';basket.textContent='Basket (fixture)';panel.append(library,basket);shell.append(panel);panel.addEventListener('sl-reposition',measure)};
new ResizeObserver(measure).observe(document.querySelector('.fixture-shell'));
document.querySelector('#browse-mode-bar').addEventListener('click',()=>setTimeout(measure,100));
await initLibraryView();controls();
</script></html>`);
let server;
async function cleanup(){await server?.close();for(const file of files){try{unlinkSync(file)}catch(error){if(error.code!=='ENOENT')throw error}}process.exit();}
process.on('SIGINT',cleanup);process.on('SIGTERM',cleanup);

try { server=await createServer({configFile:'hifimule-ui/vite.config.ts',server:{port:1422}});await server.listen();server.printUrls(); } catch(error) { for(const file of files){try{unlinkSync(file)}catch(error){if(error.code!=='ENOENT')throw error}}throw error; }
