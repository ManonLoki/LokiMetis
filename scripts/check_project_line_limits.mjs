#!/usr/bin/env node
/** 在原样上游行数门禁之上保留本项目封存物与第三方字节的严格登记。 */
import { readFileSync } from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import {
  canonicalRoot, gitVisiblePaths, inspectRepository as inspectUpstream, main as upstreamMain,
} from '../.agents/skills/desktop-implement-change/scripts/check_file_line_limits.mjs';
import { verifyRegisteredGenerated } from '../.agents/skills/desktop-implement-change/scripts/verified_generated.mjs';
import { verifyRegisteredThirdParty } from '../.agents/skills/desktop-implement-change/scripts/verified_third_party.mjs';

/** 只从已完成上游扫描的结果中移除精确验证的原样文件，任何扫描或登记错误仍阻断。 */
export function inspectRepository(root) {
  const report = inspectUpstream(root);
  report.excludedThirdPartyFiles = [];
  const [canonical] = canonicalRoot(root);
  if (canonical === null) return report;
  const [visible, errors] = gitVisiblePaths(canonical);
  if (errors.length) {
    // 上游同一枚举失败已记录；只补充本次新出现的错误，避免重复诊断。
    report.errors.push(...errors.filter(error => !report.errors.includes(error)));
    report.ok = false;
    return report;
  }
  const visiblePaths = new Set(visible);
  const generated = verifyRegisteredGenerated(canonical);
  const thirdParty = verifyRegisteredThirdParty(canonical);
  const generatedPaths = generated.files.filter(file => visiblePaths.has(file));
  report.excludedThirdPartyFiles = thirdParty.files.filter(file => visiblePaths.has(file));
  const excluded = new Set([...generatedPaths, ...report.excludedThirdPartyFiles]);
  const alreadyExcluded = new Set(report.excludedGeneratedFiles);
  for (const relative of excluded) {
    if (alreadyExcluded.has(relative)) continue;
    // 与上游 readText 的文本判定一致：无 NUL 且严格 UTF-8 才曾计入 checkedTextFiles。
    let payload;
    try { payload = readFileSync(path.join(canonical, relative)); } catch { continue; /* 登记校验会报告缺失或不可读。 */ }
    if (!payload.includes(0)) {
      try {
        new TextDecoder('utf-8', { fatal: true }).decode(payload);
        report.checkedTextFiles -= 1;
      } catch { /* 上游 UTF-8 错误必须保留，不能由登记豁免。 */ }
    }
  }
  const thirdPartyPaths = new Set(report.excludedThirdPartyFiles);
  report.excludedGeneratedFiles = [...new Set([
    ...report.excludedGeneratedFiles.filter(file => !thirdPartyPaths.has(file)), ...generatedPaths,
  ])].sort();
  report.excludedThirdPartyFiles.sort();
  report.violations = report.violations.filter(item => !excluded.has(item.path));
  report.reviewCandidates = report.reviewCandidates.filter(item => !excluded.has(item.path));
  report.errors.push(...generated.errors, ...thirdParty.errors);
  report.errors.sort();
  report.ok = report.errors.length === 0 && report.violations.length === 0;
  return report;
}

/** 复用上游命令参数、诊断和退出码，不另建阈值或扫描规则。 */
export function main(args = process.argv.slice(2)) { return upstreamMain(args, inspectRepository); }

if (path.resolve(process.argv[1] ?? '') === fileURLToPath(import.meta.url)) {
  try { process.exitCode = main(); }
  catch (error) { process.stderr.write(`ERROR: ${error.message}\n`); process.exitCode = 2; }
}
