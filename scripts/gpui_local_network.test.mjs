/** 本地网络隐私声明同时进入预览与正式包，不执行实际构建或系统授权。 */
import assert from 'node:assert/strict';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import test from 'node:test';
import { renderPreviewPlist } from './gpui_preview.mjs';
import { readConfig, validateConfig } from '../.agents/skills/desktop-build-gpui-release/scripts/gpui_config.mjs';
import { macosAgentPlist } from '../.agents/skills/desktop-build-gpui-release/scripts/gpui_macos_identity.mjs';
import { renderLocalNetworkPlist } from '../.agents/skills/desktop-build-gpui-release/scripts/gpui_macos_network.mjs';

test('preview and package declare exactly the approved receiver service', t => {
  const root = fileURLToPath(new URL('../', import.meta.url));
  const config = readConfig(root);
  const directory = fs.mkdtempSync(path.join(os.tmpdir(), 'loki-network-plist-'));
  t.after(() => fs.rmSync(directory, { recursive: true }));
  const fragment = renderLocalNetworkPlist(config.macos.localNetwork);
  assert.match(fragment, /<string>_loki-metris\._tcp<\/string>/u);
  assert.ok(renderPreviewPlist(config, { version: '0.6.0' }).includes(fragment));
  const plist = macosAgentPlist(directory, true, config.macos.localNetwork);
  assert.ok(fs.readFileSync(plist, 'utf8').includes(fragment));
  assert.equal(renderLocalNetworkPlist(undefined), '');
});

test('unsafe network metadata is rejected while XML content is escaped', () => {
  for (const value of [null, {}, { usageDescription: 'test', bonjourServices: ['_http._tcp', '_http._tcp'] },
    { usageDescription: 'test', bonjourServices: ['*'] }, { usageDescription: '', bonjourServices: ['_loki-metris._tcp'] }]) {
    assert.throws(() => renderLocalNetworkPlist(value));
  }
  assert.match(renderLocalNetworkPlist({ usageDescription: '<device> & app', bonjourServices: ['_loki-metris._tcp'] }), /&lt;device&gt; &amp; app/u);
  const config = readConfig(fileURLToPath(new URL('../', import.meta.url)));
  const legacy = structuredClone(config); delete legacy.macos.localNetwork;
  assert.doesNotThrow(() => validateConfig(legacy));
  config.macos.localNetwork.bonjourServices = ['_bad<key>._tcp'];
  assert.throws(() => validateConfig(config));
});
