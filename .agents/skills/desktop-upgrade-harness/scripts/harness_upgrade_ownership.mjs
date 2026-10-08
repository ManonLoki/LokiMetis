/** Harness 升级所有权、文件树和共同基线读取。 */

import fs from "node:fs";
import path from "node:path";
import { spawnSync } from "node:child_process";
import { MINIMUM_OWNERSHIP_RULES, REQUIRED_MANAGED_SOURCE_PATHS, SCHEMA_VERSION, VALID_MODES } from "./harness_upgrade_policy.mjs";
import { UpgradeError, loadJson, lstatOrNull, safeRelativePath, snapshotFile, validateSnapshot } from "./harness_upgrade_safety.mjs";

function globRegex(pattern) {
  let source = "^";
  for (let index = 0; index < pattern.length; index += 1) {
    const character = pattern[index];
    if (character === "*") { while (pattern[index + 1] === "*") index += 1; source += ".*"; }
    else if (character === "?") source += ".";
    else if (character === "[") {
      let closing = index + 1;
      if (pattern[closing] === "!") closing += 1;
      if (pattern[closing] === "]") closing += 1;
      while (closing < pattern.length && pattern[closing] !== "]") closing += 1;
      if (closing >= pattern.length) source += "\\[";
      else {
        let content = pattern.slice(index + 1, closing);
        const negated = content.startsWith("!");
        if (negated) content = content.slice(1);
        content = content.replaceAll("\\", "\\\\").replaceAll("]", "\\]").replaceAll("[", "\\[");
        if (content.startsWith("^")) content = `\\${content}`;
        const characterClass = `[${negated ? "^" : ""}${content}]`;
        try { new RegExp(characterClass, "u"); source += characterClass; } catch { source += "(?!)"; }
        index = closing;
      }
    }
    else source += character.replace(/[\\^$.*+?()[\]{}|]/gu, "\\$&");
  }
  return new RegExp(`${source}$`, "u");
}

export function matchesPattern(value, pattern) { return globRegex(pattern).test(value); }

/** 旧清单若已由通用 Skill 规则赋予相同 managed 所有权，可先规划并在记录前修复。 */
function recoverableManagedOmission(pattern, mode, rules) {
  if (mode !== "managed" || !pattern.startsWith(".agents/skills/")) return false;
  const scope = pattern.endsWith("/**") ? pattern.slice(0, -3) : pattern;
  if ([...scope].some((character) => "*?[".includes(character))) return false;
  const genericIndex = rules.findIndex(([rule, ruleMode]) => rule === ".agents/skills/**" && ruleMode === "managed");
  if (genericIndex < 0) return false;
  for (const [rule, ruleMode] of rules.slice(0, genericIndex)) {
    if (ruleMode === "managed") continue;
    const wildcardIndex = [...rule].findIndex((character) => "*?[".includes(character));
    const fixedPrefix = (wildcardIndex < 0 ? rule : rule.slice(0, wildcardIndex)).replace(/\/$/u, "");
    if (!fixedPrefix || scope === fixedPrefix || scope.startsWith(`${fixedPrefix}/`) || fixedPrefix.startsWith(`${scope}/`)
      || (wildcardIndex >= 0 && scope.startsWith(fixedPrefix))) return false;
  }
  return ownershipMode(scope, "protected", rules) === "managed"
    && ownershipMode(`${scope}/__harness_node_probe__`, "protected", rules) === "managed";
}

/** 加载有序所有权规则；具体规则必须位于兜底规则之前。 */
export function loadOwnership(file, { allowRecoverableManagedOmissions = false } = {}) {
  const data = loadJson(file, "所有权 manifest");
  if (data.schema_version !== 1) throw new UpgradeError("所有权 manifest 的 schema_version 必须为 1");
  if (data.default_mode !== "protected") throw new UpgradeError("所有权 manifest 的 default_mode 必须为 protected");
  if (!Array.isArray(data.rules) || !data.rules.length) throw new UpgradeError("所有权 manifest 的 rules 必须是非空列表");
  const rules = [];
  const seen = new Set();
  data.rules.forEach((item, index) => {
    if (!item || Array.isArray(item) || typeof item !== "object" || Object.keys(item).sort().join(",") !== "mode,pattern") throw new UpgradeError(`所有权规则 ${index} 必须包含 pattern/mode`);
    if (typeof item.pattern !== "string" || !item.pattern) throw new UpgradeError(`所有权规则 ${index} 的 pattern 非法`);
    if (seen.has(item.pattern)) throw new UpgradeError(`所有权规则的 pattern 重复：${item.pattern}`);
    if (!VALID_MODES.has(item.mode)) throw new UpgradeError(`所有权规则 ${index} 的 mode 非法：${JSON.stringify(item.mode)}`);
    seen.add(item.pattern); rules.push([item.pattern, item.mode]);
  });
  const observed = new Map(rules);
  const missingManagedRules = [];
  for (const [pattern, mode] of MINIMUM_OWNERSHIP_RULES) {
    if (observed.get(pattern) === mode) continue;
    if (allowRecoverableManagedOmissions && !observed.has(pattern) && recoverableManagedOmission(pattern, mode, rules)) {
      missingManagedRules.push(pattern);
      continue;
    }
    throw new UpgradeError(`所有权 manifest 削弱了必需保护：${pattern} 必须为 ${mode}`);
  }
  const selfIndex = rules.findIndex(([pattern, mode]) => pattern === ".agents/skills/desktop-upgrade-harness/**" && mode === "managed-self");
  const genericIndex = rules.findIndex(([pattern, mode]) => pattern === ".agents/skills/**" && mode === "managed");
  const designIndex = rules.findIndex(([pattern, mode]) => pattern === ".agents/skills/design-taste-frontend/**" && mode === "protected");
  if (designIndex < 0 || designIndex >= genericIndex) throw new UpgradeError("第三方设计 Skill 的 protected 规则必须位于通用 managed 规则之前");
  const designScope = ".agents/skills/design-taste-frontend";
  for (const [pattern, mode] of rules.slice(0, designIndex)) {
    if (mode === "protected") continue;
    const wildcard = [...pattern].findIndex((character) => "*?[".includes(character));
    const prefix = (wildcard < 0 ? pattern : pattern.slice(0, wildcard)).replace(/\/$/u, "");
    if (!prefix || designScope === prefix || designScope.startsWith(`${prefix}/`)
        || prefix.startsWith(`${designScope}/`) || (wildcard >= 0 && designScope.startsWith(prefix))) {
      throw new UpgradeError(`第三方设计 Skill 的 protected 规则被更早的所有权规则遮蔽：${pattern}`);
    }
  }
  if (selfIndex < 0 || genericIndex < 0 || selfIndex >= genericIndex) throw new UpgradeError("managed-self 所有权规则必须位于通用 managed 规则之前");
  for (const [pattern, mode] of MINIMUM_OWNERSHIP_RULES) {
    if (mode !== "conditional") continue;
    const index = rules.findIndex(([rule, ruleMode]) => rule === pattern && ruleMode === mode);
    if (index >= genericIndex) throw new UpgradeError(`conditional 所有权规则必须位于通用 managed 规则之前：${pattern}`);
  }
  const gpuiIndex = rules.findIndex(([pattern]) => pattern === ".agents/skills/desktop-add-gpui-adapter/**");
  for (const [pattern, mode] of MINIMUM_OWNERSHIP_RULES) {
    if (mode !== "tombstone" || !pattern.startsWith(".agents/skills/desktop-add-gpui-adapter/")) continue;
    const index = rules.findIndex(([rule]) => rule === pattern);
    const probe = pattern.endsWith("/**") ? `${pattern.slice(0, -3)}/__harness_node_probe__` : pattern;
    if (index >= gpuiIndex || ownershipMode(probe, data.default_mode, rules) !== "tombstone") {
      throw new UpgradeError(`GPUI 初始化专用 tombstone 规则必须位于 GPUI conditional 规则之前且不得被遮蔽：${pattern}`);
    }
  }
  for (const required of REQUIRED_MANAGED_SOURCE_PATHS) if (ownershipMode(required, data.default_mode, rules) !== "managed") throw new UpgradeError(`必需传播路径的有效所有权必须保持 managed：${required}`);
  return { defaultMode: data.default_mode, rules, missingManagedRules };
}

export function ownershipMode(value, defaultMode, rules) {
  for (const [pattern, mode] of rules) if (matchesPattern(value, pattern)) return mode;
  return defaultMode;
}

export function explicitOwnershipModeForNode(value, { isDirectory, rules }) {
  if (isDirectory) for (const [pattern, mode] of rules) if (matchesPattern(`${value}/__harness_node_probe__`, pattern)) return mode;
  for (const [pattern, mode] of rules) if (matchesPattern(value, pattern)) return mode;
  return null;
}

function relativePosix(root, value) { return path.relative(root, value).split(path.sep).join("/"); }

const GENERATED_DIRECTORY_NAMES = new Set(["node_modules", "target", "dist", "coverage", ".vite", ".cache", "release"]);

/** 枚举所有节点及普通文件快照，并报告链接、特殊文件和候选 Git 元数据。 */
export function scanTree(root, { targetTree, retainPaths = [] }) {
  const files = {};
  const unsafe = [];
  const nodes = {};
  const retain = [...new Set(retainPaths)];
  const canPrune = (relative) => targetTree && GENERATED_DIRECTORY_NAMES.has(path.posix.basename(relative))
    && !retain.some((required) => required === relative || required.startsWith(`${relative}/`))
    && spawnSync("git", ["-C", root, "check-ignore", "-q", "--", relative], { encoding: "utf8" }).status === 0;
  const visit = (current) => {
    const entries = fs.readdirSync(current, { withFileTypes: true }).sort((a, b) => a.name.localeCompare(b.name, "en"));
    for (const entry of entries) {
      const child = path.join(current, entry.name);
      const relative = relativePosix(root, child);
      if (targetTree && relative === ".git") continue;
      const observed = fs.lstatSync(child);
      nodes[relative] = entry.isDirectory();
      if (observed.isSymbolicLink()) { unsafe.push(relative); continue; }
      if (observed.isDirectory()) {
        if (!targetTree && relative === ".git") unsafe.push(relative);
        else if (!canPrune(relative)) visit(child);
      } else if (!observed.isFile()) unsafe.push(relative);
      else files[relative] = snapshotFile(child);
    }
  };
  visit(root);
  return { files, unsafe: unsafe.sort(), nodes };
}

/** 读取并严格校验上一版来源锁；缺失表示必须走 bootstrap audit。 */
export function loadLock(file) {
  if (!lstatOrNull(file)) return null;
  const data = loadJson(file, "上游 lock");
  if (data.schema_version !== SCHEMA_VERSION) throw new UpgradeError(`上游 lock 的 schema_version 必须为 ${SCHEMA_VERSION}`);
  if (!data.entries || Array.isArray(data.entries) || typeof data.entries !== "object") throw new UpgradeError("上游 lock 的 entries 必须是 object");
  for (const [rawPath, entry] of Object.entries(data.entries)) {
    if (safeRelativePath(rawPath) !== rawPath || !entry || Array.isArray(entry) || typeof entry !== "object") throw new UpgradeError(`上游 lock entry 非法：${JSON.stringify(rawPath)}`);
    if (Object.keys(entry).sort().join(",") !== "candidate,mode,target") throw new UpgradeError(`${rawPath} 的上游 lock 字段非法`);
    if (!VALID_MODES.has(entry.mode)) throw new UpgradeError(`${rawPath} 的上游 lock mode 非法`);
    validateSnapshot(entry.candidate, `${rawPath}.candidate`); validateSnapshot(entry.target, `${rawPath}.target`);
  }
  return data;
}
