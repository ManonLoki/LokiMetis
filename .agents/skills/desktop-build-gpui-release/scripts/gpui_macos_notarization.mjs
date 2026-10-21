/** 正式 macOS 候选的 Developer ID、公证与最终票据核验；凭据只来自显式调用环境。 */
import fs from 'node:fs';
import path from 'node:path';

/** 仅显式启用的调用可把专用认证映射给 cargo-packager，构建和测试不接触认证。 */
export function notarizationEnvironment(config, environment) {
  const identity = config.macos.signingIdentity;
  if (!/^Developer ID Application: .+ \([A-Z0-9]{10}\)$/u.test(identity ?? '')) throw new Error('macOS signing requires an approved Developer ID identity');
  const key = environment.LM_NOTARIZATION_KEY_PATH;
  if (!key || !path.isAbsolute(key) || !fs.lstatSync(key).isFile() || fs.lstatSync(key).isSymbolicLink()) throw new Error('macOS signing requires a regular notarization key file');
  if (!/^[A-Z0-9]{10}$/u.test(environment.LM_NOTARIZATION_KEY_ID ?? '') || !/^[a-f0-9-]{36}$/u.test(environment.LM_NOTARIZATION_ISSUER ?? '')) throw new Error('macOS signing requires notarization key ID and issuer');
  return { ...environment, APPLE_API_KEY_PATH: key, APPLE_API_KEY: environment.LM_NOTARIZATION_KEY_ID, APPLE_API_ISSUER: environment.LM_NOTARIZATION_ISSUER };
}

/** 签名必须绑定公开身份、团队、安全时间戳；应用还必须有 hardened runtime 和正确 identifier。 */
export function verifyDeveloperSignature(run, artifact, config, env, application = true) {
  run('/usr/bin/codesign', ['--verify','--deep','--strict','--verbose=4',artifact], { env, quiet: true });
  const result = run('/usr/bin/codesign', ['-dv','--verbose=4',artifact], { env, quiet: true });
  const details = `${result.stdout}\n${result.stderr}`, team = config.macos.signingIdentity.match(/\(([A-Z0-9]{10})\)$/u)[1];
  if (!details.includes(`Authority=${config.macos.signingIdentity}\n`) || !details.includes(`TeamIdentifier=${team}\n`) || !/^Timestamp=.+$/mu.test(details)) throw new Error('Developer ID, team or secure timestamp mismatch');
  if (application && (!/^CodeDirectory .*flags=.*runtime.*$/mu.test(details) || !details.includes(`Identifier=${config.identifier}\n`))) throw new Error('Application hardened runtime or identifier mismatch');
  const cdhash = details.match(/^CDHash=([a-f0-9]+)$/mu)?.[1];
  if (application && !cdhash) throw new Error('Signed application CDHash is missing');
  return { authority: config.macos.signingIdentity, teamIdentifier: team, secureTimestamp: true, strictVerification: true, ...(application ? { hardenedRuntime: true, cdhash } : {}) };
}

/** 查询 Apple 的真实回执及签名票据，拒绝只有日志字符串或非 Accepted 状态的结果。 */
function acceptedReceipt(run, id, authentication, env) {
  if (!/^[a-f0-9-]{36}$/u.test(id ?? '')) throw new Error('Missing notarization submission ID');
  const info = JSON.parse(run('xcrun', ['notarytool','info',id,...authentication,'--output-format','json'], { env, quiet: true }).stdout);
  if (info.id !== id || info.status !== 'Accepted') throw new Error('Apple notarization is not Accepted');
  const log = JSON.parse(run('xcrun', ['notarytool','log',id,...authentication], { env, quiet: true }).stdout);
  if (log.jobId !== id || log.status !== 'Accepted') throw new Error('Apple notarization log differs from submission');
  return { id, status: info.status, log };
}

/** 上游先公证并附票 App；DMG 再独立公证附票，最终 Gatekeeper 校验后才能计算候选摘要。 */
export function finalizeMacosSigning({ run, config, format, artifact, app, env, packageResult }) {
  const authentication = ['--key',env.APPLE_API_KEY_PATH,'--key-id',env.APPLE_API_KEY,'--issuer',env.APPLE_API_ISSUER];
  const ids = [...`${packageResult.stdout}\n${packageResult.stderr}`.matchAll(/status Accepted for id ([a-f0-9-]{36})/gu)].map(match => match[1]);
  if (new Set(ids).size !== 1) throw new Error('Expected one accepted application notarization receipt from packager');
  const signature = verifyDeveloperSignature(run, app, config, env);
  const application = acceptedReceipt(run, ids[0], authentication, env);
  if (!application.log.ticketContents?.some(item => item.cdhash === signature.cdhash)) throw new Error('Notarization ticket does not contain this signed application');
  run('xcrun', ['stapler','validate',app], { env, quiet: true });
  const policy = run('spctl',['--status'],{ env, quiet: true }).stdout.trim();
  if (policy !== 'assessments enabled') throw new Error('Gatekeeper assessments must be enabled for candidate verification');
  run('spctl', ['--assess','--type','execute','--verbose=4',app], { env, quiet: true });
  run('syspolicy_check',['distribution',app],{ env, quiet: true });
  let dmg;
  if (format === 'dmg') {
    verifyDeveloperSignature(run, artifact, config, env, false);
    const submission = JSON.parse(run('xcrun', ['notarytool','submit',artifact,...authentication,'--wait','--timeout','30m','--output-format','json'], { env, quiet: true, timeout: 31 * 60_000 }).stdout);
    dmg = acceptedReceipt(run, submission.id, authentication, env);
    run('xcrun', ['stapler','staple',artifact], { env, quiet: true });
    run('xcrun', ['stapler','validate',artifact], { env, quiet: true });
    verifyDeveloperSignature(run, artifact, config, env, false);
    run('spctl', ['--assess','--type','open','--context','context:primary-signature','--verbose=4',artifact], { env, quiet: true });
  }
  return { signingStatus: 'signed', notarizationStatus: 'accepted-stapled', signingEvidence: { status: 'verified-developer-id', ...signature }, notarizationEvidence: { application, ...(dmg ? { dmg } : {}), stapling: 'validated', gatekeeper: 'accepted', gatekeeperPolicy: policy, distributionPolicy: 'passed' } };
}
