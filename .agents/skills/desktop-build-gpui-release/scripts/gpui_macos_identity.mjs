/** macOS 本机试包的完整 bundle 身份；不读取证书、不公证、不表示渠道信任。 */
import fs from 'node:fs';
import path from 'node:path';
import { sha256 } from './gpui_filesystem.mjs';
import { renderLocalNetworkPlist } from './gpui_macos_network.mjs';

/** 完整应用必须绑定正确 identifier、Info.plist 和资源封印，不能接受仅 Mach-O 链接签名。 */
export function verifyLocalBundle(run, app, identifier, env) {
  run('/usr/bin/codesign', ['--verify', '--strict', '--verbose=4', app], { env, quiet: true, timeout: 60_000 });
  const result = run('/usr/bin/codesign', ['-dv', '--verbose=4', app], { env, quiet: true, timeout: 10_000 });
  const details = `${result.stdout}\n${result.stderr}`;
  if (details.match(/^Identifier=(.+)$/mu)?.[1] !== identifier || !/^Signature=adhoc$/mu.test(details)
      || !/^Info\.plist entries=\d+$/mu.test(details) || !/^Sealed Resources version=\d+/mu.test(details)) {
    throw new Error('local macOS bundle lacks the expected bound ad-hoc identity');
  }
}

/** 明确仅以本地无证书方式签完整应用；失败时不得输出可用试包。 */
export function signLocalBundle(run, app, identifier, env) {
  run('/usr/bin/codesign', ['--force', '--sign', '-', app], { env, quiet: true, timeout: 60_000 });
  verifyLocalBundle(run, app, identifier, env);
}

/** HookCommand 由上游经 shell 运行，使用 POSIX 单引号逐项保护路径。 */
function shellArgument(value) { return `'${value.replaceAll("'", "'\\''")}'`; }

/** 固定内部 hook 仅在 DMG 封盘前签已生成的 App，不能执行项目配置中的任意脚本。 */
export function signLocalDmgApp(run, app, identifier, binary, expectedHash, receipt, env) {
  if (sha256(binary) !== expectedHash) throw new Error('app bundle binary differs before local signing');
  signLocalBundle(run, app, identifier, env);
  fs.writeFileSync(receipt, JSON.stringify({ before: expectedHash, after: sha256(binary) }), { flag: 'wx' });
}

/** 在已生成 App 与 DMG 封盘之间验证原始二进制并保存签名后的字节回执。 */
export function localDmgSigningHook(work, app, identifier, binaryName, expectedHash) {
  const hook = path.join(work, 'local-dmg-sign.mjs');
  const source = `import { spawnSync } from 'node:child_process';
import { signLocalDmgApp } from ${JSON.stringify(import.meta.url)};
if (process.env.CARGO_PACKAGER_FORMAT === 'dmg') {
  const run = (program, args, options = {}) => {
    const result = spawnSync(program, args, { encoding: 'utf8', timeout: options.timeout, env: options.env });
    if (result.error || result.status !== 0) throw new Error('local bundle signing or verification failed');
    return result;
  };
  signLocalDmgApp(run, ${JSON.stringify(app)}, ${JSON.stringify(identifier)}, ${JSON.stringify(path.join(app, 'Contents/MacOS', binaryName))}, ${JSON.stringify(expectedHash)}, ${JSON.stringify(path.join(work, 'local-dmg-identity.json'))}, process.env);
}
`;
  fs.writeFileSync(hook, source, { flag: 'wx' });
  return `${shellArgument(process.execPath)} ${shellArgument(hook)}`;
}

/** Dock 代理身份在打包时固化，和运行时的 Accessory 策略一致。 */
export function macosAgentPlist(work, agent, localNetwork) {
  const plist = path.join(work, 'macos-info.plist');
  fs.writeFileSync(plist, `<?xml version="1.0" encoding="UTF-8"?>\n<plist version="1.0"><dict><key>LSUIElement</key><${agent ? 'true' : 'false'}/>\n${renderLocalNetworkPlist(localNetwork)}</dict></plist>\n`);
  return plist;
}
