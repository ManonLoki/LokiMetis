import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import path from "node:path";
import { spawnSync } from "node:child_process";
import test from "node:test";
import { fileURLToPath } from "node:url";

import { auditWorkBuddyAdapter } from "./audit_component_map.mjs";

const scriptDirectory = path.dirname(fileURLToPath(import.meta.url));
const skillDirectory = path.resolve(scriptDirectory, "..");
const repositoryDirectory = path.resolve(skillDirectory, "..", "..", "..");
const mapPath = path.join(skillDirectory, "references", "component-map.yaml");
const adapterPath = path.join(
  repositoryDirectory,
  "loki_metis_gui",
  "resources",
  "workbuddy-skin-host-compat.js",
);
const auditScriptPath = path.join(scriptDirectory, "audit_component_map.mjs");
const templatePath = path.join(skillDirectory, "assets", "skin-template");

const [mapText, adapterSource] = await Promise.all([
  readFile(mapPath, "utf8"),
  readFile(adapterPath, "utf8"),
]);

test("accepts_the_current_WorkBuddy_adapter_and_component_map", () => {
  assert.deepEqual(auditWorkBuddyAdapter(mapText, adapterSource), []);
});

test("reports_an_unregistered_WorkBuddy_native_selector", () => {
  const changedAdapter = adapterSource.replace(
    'shell: ".teams-container",',
    'shell: ".teams-container, .unregistered-workbuddy-node",',
  );
  assert.notEqual(changedAdapter, adapterSource);

  assert.ok(
    auditWorkBuddyAdapter(mapText, changedAdapter).includes(
      "WorkBuddy 宿主选择器原子未登记到 kind=workbuddy-native：.unregistered-workbuddy-node",
    ),
  );
});

test("rejects_compatibility_aliases_registered_as_WorkBuddy_native", () => {
  const changedMap = mapText.replace(
    /  - component_id: workbuddy\.compatibility-aliases\r?\n    kind: skin-state/,
    "  - component_id: workbuddy.compatibility-aliases\n    kind: workbuddy-native",
  );
  assert.notEqual(changedMap, mapText);

  assert.ok(
    auditWorkBuddyAdapter(changedMap, adapterSource).includes(
      "WorkBuddy 兼容状态选择器不得登记为 workbuddy-native：.loki-metis-workbuddy-shell",
    ),
  );
});

test("reports_a_missing_composer_mapping_in_the_adapter", () => {
  const changedAdapter = adapterSource.replace(
    "  --cb-main-area-box-shadow: none !important;\n",
    "",
  );
  assert.notEqual(changedAdapter, adapterSource);

  assert.ok(
    auditWorkBuddyAdapter(mapText, changedAdapter).includes(
      "WorkBuddy composer 适配器缺少宿主变量映射：--cb-main-area-box-shadow",
    ),
  );
});

test("reports_a_missing_composer_mapping_in_the_component_map", () => {
  const changedMap = mapText.replace(
    '      - target: "--cb-main-area-border-color"',
    '      - target: "--removed-main-area-border-color"',
  );
  assert.notEqual(changedMap, mapText);

  assert.ok(
    auditWorkBuddyAdapter(changedMap, adapterSource).includes(
      "WorkBuddy composer 宿主变量未登记到 skin-state token_mappings：--cb-main-area-border-color",
    ),
  );
});

test("rejects_a_composer_mapping_back_to_a_WorkBuddy_token", () => {
  const changedAdapter = adapterSource.replace(
    "--cb-main-area-background: transparent !important;",
    "--cb-main-area-background: var(--wb-bg-primary) !important;",
  );
  assert.notEqual(changedAdapter, adapterSource);

  assert.ok(
    auditWorkBuddyAdapter(mapText, changedAdapter).includes(
      "WorkBuddy composer 宿主变量未映射到纯主题 token：--cb-main-area-background",
    ),
  );
});

test("executes_the_component_map_CLI_on_Windows", () => {
  const result = spawnSync(process.execPath, [auditScriptPath, templatePath], {
    encoding: "utf8",
  });

  assert.equal(result.status, 0, result.stderr);
  assert.match(result.stdout, /选择器 Map 审计通过/);
});
