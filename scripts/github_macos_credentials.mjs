/** 短期 runner 钥匙串，只从 Actions secrets 导入既有证书；日志不记录秘密。 */
import fs from 'node:fs';
import path from 'node:path';
import { randomBytes } from 'node:crypto';
import { spawnSync } from 'node:child_process';

const temporary = process.env.RUNNER_TEMP;
if (!temporary || process.platform !== 'darwin') throw new Error('Native macOS Actions runner required');
const keychain = path.join(temporary,'loki-signing.keychain-db');
const certificate = path.join(temporary,'loki-certificate.p12');
const key = path.join(temporary,'loki-notarization.p8');

/** 签名工具错误只输出类别；禁止把参数、凭据或环境回显进 CI 日志。 */
function security(args) {
  const result = spawnSync('/usr/bin/security',args,{ encoding: 'utf8',timeout: 60_000 });
  if (result.error || result.status !== 0) {
    let diagnostic = result.stderr?.trim() || 'No diagnostic';
    for (const name of ['MACOS_CERTIFICATE_BASE64','MACOS_CERTIFICATE_PASSWORD','APPLE_NOTARIZATION_KEY','APPLE_NOTARIZATION_KEY_ID','APPLE_NOTARIZATION_ISSUER']) {
      if (process.env[name]) diagnostic = diagnostic.replaceAll(process.env[name],'***');
    }
    throw new Error(`Temporary signing keychain ${args[0]} failed: ${diagnostic.slice(-500)}`);
  }
  return result;
}

if (process.argv[2] === 'import') {
  for (const name of ['MACOS_CERTIFICATE_BASE64','MACOS_CERTIFICATE_PASSWORD','APPLE_NOTARIZATION_KEY','APPLE_NOTARIZATION_KEY_ID','APPLE_NOTARIZATION_ISSUER']) if (!process.env[name]) throw new Error(`Missing Actions secret: ${name}`);
  if (process.arch !== 'arm64') throw new Error('Expected native Apple Silicon runner');
  const password = randomBytes(32).toString('hex');
  fs.writeFileSync(certificate,Buffer.from(process.env.MACOS_CERTIFICATE_BASE64,'base64'),{ mode: 0o600,flag:'wx' });
  fs.writeFileSync(key,process.env.APPLE_NOTARIZATION_KEY,{ mode: 0o600,flag:'wx' });
  security(['create-keychain','-p',password,keychain]);
  security(['set-keychain-settings','-lut','21600',keychain]);
  security(['unlock-keychain','-p',password,keychain]);
  security(['import',certificate,'-k',keychain,'-P',process.env.MACOS_CERTIFICATE_PASSWORD,'-T','/usr/bin/codesign','-T','/usr/bin/security']);
  security(['set-key-partition-list','-S','apple-tool:,apple:,codesign:','-s','-k',password,keychain]);
  security(['list-keychains','-d','user','-s',keychain]);
  const identity = JSON.parse(fs.readFileSync('packaging/gpui.json','utf8')).macos.signingIdentity;
  const identities = security(['find-identity','-v','-p','codesigning',keychain]);
  if (!identities.stdout.includes(`"${identity}"`)) throw new Error('Approved Developer ID identity is unavailable in the temporary keychain');
  fs.unlinkSync(certificate);
  fs.appendFileSync(process.env.GITHUB_ENV,`LM_NOTARIZATION_KEY_PATH=${key}\nLM_NOTARIZATION_KEY_ID=${process.env.APPLE_NOTARIZATION_KEY_ID}\nLM_NOTARIZATION_ISSUER=${process.env.APPLE_NOTARIZATION_ISSUER}\n`);
} else if (process.argv[2] === 'cleanup') {
  if (fs.existsSync(keychain)) security(['delete-keychain',keychain]);
  for (const file of [certificate,key]) fs.rmSync(file,{ force: true });
} else throw new Error('Expected import or cleanup');
