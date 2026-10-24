// 标准库VM执行由Rust生产代码生成的脚本；不连接浏览器或用户会话。
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import vm from 'node:vm';

const scripts = JSON.parse(readFileSync(0, 'utf8'));
const OWN = '__LOKI_METRIS_DREAM_SKIN_STATE__';
const COMPAT = '__LOKI_METRIS_CODEX_SKIN_COMPAT__';
const FOREIGN = '__CODEX_DREAM_SKIN_STATE__';
const FOREIGN_COMPAT = '__BIFANG_CODEX_SKIN_COMPAT__';

// 有状态的DOM与回调登记表让清理结果可观察，保留旧回调供强制重放。
function fixture(host = 'codex') {
  const nodes = [];
  const callbacks = [];
  const timers = new Map();
  const listeners = new Map();
  const urls = new Set();
  const observers = [];
  let nextId = 0;
  class Element {
    constructor(tag = 'DIV') {
      this.tagName = tag.toUpperCase(); this.nodeType = 1; this.id = '';
      this.isConnected = true; this.children = []; this.dataset = {};
      this.attributes = new Map(); this.classes = new Set(); this.properties = new Map();
      this.classList = { add: (name) => this.classes.add(name), remove: (name) => this.classes.delete(name), contains: (name) => this.classes.has(name) };
      this.style = { setProperty: (name, value) => this.properties.set(name, value), removeProperty: (name) => this.properties.delete(name) };
      this.sheet = { cssRules: [{}] }; this.textContent = '';
      nodes.push(this);
    }
    get className() { return [...this.classes].join(' '); }
    set className(value) { this.classes = new Set(value.split(/\s+/).filter(Boolean)); }
    appendChild(node) { this.children.push(node); node.isConnected = true; return node; }
    remove() { this.isConnected = false; }
    replaceChildren() { this.children = []; }
    setAttribute(name, value) { this.attributes.set(name, value); }
    getAttribute(name) { return this.attributes.get(name) ?? null; }
    removeAttribute(name) { this.attributes.delete(name); }
    querySelector() { return null; }
    querySelectorAll() { return []; }
    closest() { return null; }
    matches(selector) { return this.tagName === 'MAIN' && (selector.includes('main') || selector.includes('.teams-container')); }
    getBoundingClientRect() { return { left: 0, top: 0, width: 1200, height: 800 }; }
  }
  const root = new Element('HTML'); const body = new Element('BODY');
  const head = new Element('HEAD'); const main = new Element('MAIN');
  const mount = new Element(); mount.id = 'root';
  body.dataset = { applicationName: 'workbuddy', electronDesktop: 'true', productName: 'WorkBuddy' };
  const document = {
    documentElement: root, body, head, title: host === 'codex' ? 'Codex' : 'WorkBuddy',
    createElement: (tag) => new Element(tag),
    getElementById: (id) => nodes.findLast((node) => node.id === id && node.isConnected) ?? null,
    querySelector(selector) {
      if (selector === 'main.main-surface, #root') return main;
      if (selector === '#root') return mount;
      if (host === 'workBuddy' && selector.startsWith('body[data-application-name=')) return body;
      if (selector === 'main.main-surface' && main.classList.contains('main-surface')) return main;
      return null;
    },
    querySelectorAll(selector) {
      if (['main[data-app-shell-main-surface]', '#root main', '.teams-container'].includes(selector)) return [main];
      return [];
    },
  };
  const add = (name, callback) => { if (!listeners.has(name)) listeners.set(name, new Set()); listeners.get(name).add(callback); callbacks.push(callback); };
  const remove = (name, callback) => listeners.get(name)?.delete(callback);
  const timer = (callback) => { const id = ++nextId; timers.set(id, callback); callbacks.push(callback); return id; };
  const media = { matches: false, addEventListener: (name, cb) => add(`media:${name}`, cb), removeEventListener: (name, cb) => remove(`media:${name}`, cb) };
  class Observer {
    constructor(callback) { this.callback = callback; this.disconnected = false; observers.push(this); callbacks.push(() => callback([{type:'childList', target:root, addedNodes:[], removedNodes:[]} ])); }
    observe() { this.disconnected = false; }
    disconnect() { this.disconnected = true; }
  }
  const window = {
    innerWidth: 1400, matchMedia: () => media,
    addEventListener: add, removeEventListener: remove,
    setTimeout: timer, clearTimeout: (id) => timers.delete(id),
  };
  const urlApi = { createObjectURL: () => { const value = `blob:fixture-${++nextId}`; urls.add(value); return value; }, revokeObjectURL: (value) => urls.delete(value) };
  const context = vm.createContext({ window, document, location: { search: '', hash: '', pathname: '/', protocol: host === 'codex' ? 'app:' : 'file:' }, MutationObserver: Observer, URL: urlApi, URLSearchParams, Blob, Uint8Array, atob, setTimeout: timer, clearTimeout: (id) => timers.delete(id), setInterval: timer, clearInterval: (id) => timers.delete(id) });
  const evaluate = (code) => vm.runInContext(code, context, { timeout: 1000 });
  return { evaluate, window, document, root, main, timers, listeners, urls, observers, callbacks, Element };
}

function takeOver(value) {
  let foreignCleanup = 0;
  value.window[FOREIGN] = { cleanup() { foreignCleanup += 1; } };
  value.window[FOREIGN_COMPAT] = { cleanup() { foreignCleanup += 1; } };
  value.root.classList.add('codex-dream-skin');
  value.root.dataset.dreamShell = 'foreign'; value.root.dataset.dreamSurface = 'foreign';
  value.root.style.setProperty('--skin-bg', 'foreign');
  const node = new value.Element('STYLE'); node.id = 'codex-dream-skin-style';
  return () => {
    assert.equal(foreignCleanup, 0);
    assert.equal(value.root.dataset.dreamShell, 'foreign');
    assert.equal(value.root.dataset.dreamSurface, 'foreign');
    assert.equal(value.root.properties.get('--skin-bg'), 'foreign');
    assert(value.root.classList.contains('codex-dream-skin'));
    assert(node.isConnected);
  };
}

for (const payload of scripts.payloads) {
  const preempted = fixture(); const assertForeign = takeOver(preempted);
  assert.equal(preempted.evaluate(payload.script).kind, 'foreignOwner', payload.name);
  assert.equal(preempted.evaluate(scripts.marker).kind, 'foreignOwner');
  assert.equal(preempted.window[OWN], undefined); assert.equal(preempted.timers.size, 0); assertForeign();

  const value = fixture();
  assert.equal(value.evaluate(payload.script).kind, 'applied', payload.name);
  const original = value.window[OWN]; assert(original && typeof original.ensure === 'function');
  const ownStyle = value.document.getElementById('loki-metris-dream-skin-style');
  const ownChrome = value.document.getElementById('loki-metris-dream-skin-chrome');
  assert(ownStyle?.isConnected); if (payload.name !== 'pure') assert(ownChrome?.isConnected);
  assert.equal(value.evaluate(payload.probe), true);
  value.observers[0].callback([{type:'childList', target:value.root, addedNodes:[value.main], removedNodes:[]}]);
  assert(value.timers.size > 1, 'pending runtime debounce must be exercised');
  const replay = [...value.callbacks];
  const assertTakenOver = takeOver(value);
  original.ensure();
  assert.equal(ownStyle.isConnected, false); if (ownChrome) assert.equal(ownChrome.isConnected, false);
  assert.equal(original.quiesced, true); assert.equal(value.timers.size, 0); assert.equal(value.urls.size, 0);
  assert(value.observers.every((observer) => observer.disconnected));
  for (const callback of replay) callback();
  original.cleanup(); assertTakenOver();
  assert.equal(value.evaluate(payload.probe), false); assert.equal(value.evaluate(scripts.active).activeSkin, null);
  assert.equal(value.evaluate(scripts.remove), true); assertTakenOver();
  assert.equal(value.timers.size, 0); assert.equal(value.urls.size, 0);
  assert([...value.listeners.values()].every((set) => set.size === 0));
  delete value.window[FOREIGN]; delete value.window[FOREIGN_COMPAT];
  original.cleanup(); assert.equal(value.window[OWN], undefined); assert(!value.root.classList.contains('codex-dream-skin'));

  const generations = fixture(); generations.evaluate(payload.script);
  const previous = generations.window[OWN]; const oldCallbacks = [...generations.callbacks];
  generations.evaluate(payload.script); const current = generations.window[OWN];
  const currentStyle = generations.document.getElementById('loki-metris-dream-skin-style');
  const currentProperties = [...generations.root.properties];
  const currentUrls = [...generations.urls];
  assert.notEqual(previous, current);
  previous.cleanup(); oldCallbacks.forEach((callback) => callback());
  assert.equal(generations.window[OWN], current); assert.equal(generations.window.__LOKI_METRIS_DREAM_SKIN_DISABLED__, false);
  assert.equal(generations.evaluate(payload.probe), true);
  assert.equal(generations.document.getElementById('loki-metris-dream-skin-style'), currentStyle);
  assert.deepEqual([...generations.root.properties], currentProperties);
  assert.deepEqual([...generations.urls], currentUrls);
  current.cleanup(); assert.equal(generations.timers.size, 0); assert.equal(generations.urls.size, 0);
}

for (const compat of scripts.compat) {
  const preempted = fixture(compat.host); const assertForeign = takeOver(preempted);
  assert.equal(preempted.evaluate(compat.script).kind, 'foreignOwner'); assert.equal(preempted.window[COMPAT], undefined); assertForeign();
  const value = fixture(compat.host); assert.equal(value.evaluate(compat.script).kind, 'applied');
  const state = value.window[COMPAT]; assert(state && state.ownedAliases.length > 0, `${compat.host} must exercise real aliases`);
  value.observers[0].callback([{type:'childList', target:value.root, addedNodes:[value.main], removedNodes:[]}]);
  assert(value.timers.size > 0, 'pending adapter debounce must be exercised');
  const aliases = [...state.ownedAliases]; const replay = [...value.callbacks]; const assertTakenOver = takeOver(value);
  assert.equal(value.evaluate(scripts.payloads[0].script).kind, 'foreignOwner');
  replay.forEach((callback) => callback()); state.ensure(); state.cleanup();
  assert.equal(state.quiesced, true); assert.equal(value.timers.size, 0);
  assert(value.observers.every((observer) => observer.disconnected)); assertTakenOver();
  assert(aliases.every(({node, className}) => node.classList.contains(className)));
  assert.equal(value.evaluate(scripts.remove), true); assertTakenOver();
  delete value.window[FOREIGN]; delete value.window[FOREIGN_COMPAT];
  state.cleanup(); assert.equal(value.window[COMPAT], undefined);
  assert(aliases.every(({node, className}) => !node.classList.contains(className)));

  const generations = fixture(compat.host); generations.evaluate(compat.script);
  const previous = generations.window[COMPAT]; previous.version = 'old';
  const oldCallbacks = [...generations.callbacks];
  generations.evaluate(compat.script); const current = generations.window[COMPAT];
  assert.notEqual(previous, current); previous.cleanup(); oldCallbacks.forEach((callback) => callback()); assert.equal(generations.window[COMPAT], current);
  assert(current.ownedAliases.length > 0);
  assert(current.ownedAliases.every(({node, className}) => node.classList.contains(className)));
}

const unknown = fixture(); let unknownCleanup = 0;
unknown.window[OWN] = { cleanup() { unknownCleanup += 1; } }; const assertUnknownForeign = takeOver(unknown);
assert.equal(unknown.evaluate(scripts.remove), false); assert.equal(unknownCleanup, 0); assertUnknownForeign();
console.log(`ownership fixtures passed: ${scripts.payloads.length} payloads, ${scripts.compat.length} adapters`);
