/** 用真实子进程验证失败可诊断，同时不会回显认证环境或私钥。 */
import test from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import os from 'node:os';
import { commandRunner } from './build_gpui_release.mjs';
import { processFailure, redactDiagnostic } from './gpui_process_diagnostics.mjs';

test('quiet failure retains the error while redacting each signing and GitHub credential', () => {
  const env = { ...process.env, APPLE_API_KEY:'fixture-key-id', APPLE_API_ISSUER:'fixture-issuer', LM_NOTARIZATION_KEY_PATH:'/private/fixture-secret.p8', MACOS_CERTIFICATE_PASSWORD:'fixture-password', GITHUB_TOKEN:'fixture-token' };
  const script = 'process.stderr.write("ticket unavailable " + ["APPLE_API_KEY","APPLE_API_ISSUER","LM_NOTARIZATION_KEY_PATH","MACOS_CERTIFICATE_PASSWORD","GITHUB_TOKEN"].map(name=>process.env[name]).join(" ")); process.exitCode=68';
  assert.throws(() => commandRunner(fs.realpathSync(os.tmpdir()))(process.execPath,['-e',script],{ env, quiet:true }), error => {
    assert.equal(error.exitCode,68);
    assert.match(error.message,/ticket unavailable/);
    for (const name of ['APPLE_API_KEY','APPLE_API_ISSUER','LM_NOTARIZATION_KEY_PATH','MACOS_CERTIFICATE_PASSWORD','GITHUB_TOKEN']) assert.ok(!error.message.includes(env[name]));
    return true;
  });
});

test('public xcrun action remains visible without authentication arguments', () => {
  const error = processFailure('/usr/bin/xcrun',['notarytool','log','--key','private-argument'],{ status:65,stderr:'service rejected this request',stdout:'' },{});
  assert.match(error.message,/^xcrun notarytool log failed \(exit 65\): service rejected/);
  assert.ok(!error.message.includes('private-argument'));
});

test('complete and truncated private-key blocks never survive diagnostic rendering', () => {
  // 分段构造无效夹具，避免仓库秘密扫描把静态 PEM 标题误认为真实私钥。
  const boundary = (action,type='') => ['-----',action,' ',...(type ? [type,' '] : []),'PRIVATE KEY','-----'].join('');
  for (const text of [`before\n${boundary('BEGIN')}\nfixture-data\n${boundary('END')}\nafter`,`before\n${boundary('BEGIN','EC')}\nfixture-data`]) {
    const result=redactDiagnostic(text,{});
    assert.ok(!result.includes('fixture-data'));
    assert.ok(!result.includes('-----BEGIN'));
    assert.ok(result.includes('before'));
  }
});
