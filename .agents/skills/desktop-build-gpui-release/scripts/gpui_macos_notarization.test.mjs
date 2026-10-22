/** 正式候选认证与Apple回执的失败关闭回归；不读取真实钥匙串或连接Apple。 */
import test from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { notarizationEnvironment, finalizeMacosSigning, runStapler } from './gpui_macos_notarization.mjs';

const config = { identifier:'com.example.app',macos:{signingIdentity:'Developer ID Application: Example (ABCDEFGHIJ)'} };
const appId = '11111111-1111-1111-1111-111111111111', dmgId = '22222222-2222-2222-2222-222222222222';

/** 模拟签名工具的真实输出结构，独立改变最高风险的身份、票据和策略失败。 */
function fixture(overrides = {}) {
  const calls=[];
  const run=(program,args)=>{
    calls.push([program,...args]);
    let stdout='',stderr='';
    if (program === '/usr/bin/codesign' && args[0] === '-dv') stderr=`Authority=${config.macos.signingIdentity}\nTeamIdentifier=ABCDEFGHIJ\nTimestamp=Oct 22, 2026\nCodeDirectory v=20500 flags=0x10000(runtime)\nIdentifier=${config.identifier}\nCDHash=abcdef\n`;
    if (program === 'spctl' && args[0] === '--status') stdout=overrides.policy ?? 'assessments enabled';
    if (program === 'xcrun' && args[0] === 'notarytool') {
      const id=args[2];
      if (args[1] === 'submit') stdout=JSON.stringify({id:dmgId,status:'Accepted'});
      if (args[1] === 'info') stdout=JSON.stringify({id,status:overrides.status ?? 'Accepted'});
      if (args[1] === 'log') stdout=JSON.stringify({jobId:id,status:'Accepted',ticketContents:[{cdhash:overrides.cdhash ?? 'abcdef'}]});
    }
    if (overrides.failStaple && program === 'xcrun' && args[0] === 'stapler') throw new Error('staple failed');
    return {stdout,stderr};
  };
  return {calls,options:{run,config,format:'dmg',artifact:'app.dmg',app:'App.app',env:{APPLE_API_KEY_PATH:'/private/key',APPLE_API_KEY:'ABCDEFGHIJ',APPLE_API_ISSUER:appId},packageResult:{stdout:`status Accepted for id ${appId}`,stderr:''}}};
}

test('explicit signing requires complete approved authentication before any work',t=>{
  const dir=fs.mkdtempSync(path.join(os.tmpdir(),'gpui-notary-'));t.after(()=>fs.rmSync(dir,{recursive:true,force:true}));
  const key=path.join(dir,'key.p8');fs.writeFileSync(key,'isolated fixture');
  assert.throws(()=>notarizationEnvironment(config,{}),/regular notarization/);
  const env={LM_NOTARIZATION_KEY_PATH:key,LM_NOTARIZATION_KEY_ID:'ABCDEFGHIJ',LM_NOTARIZATION_ISSUER:appId};
  assert.equal(notarizationEnvironment(config,env).APPLE_API_KEY_PATH,key);
  fs.symlinkSync(key,path.join(dir,'link.p8'));
  assert.throws(()=>notarizationEnvironment(config,{...env,LM_NOTARIZATION_KEY_PATH:path.join(dir,'link.p8')}),/regular notarization/);
});

test('both notarizations and enabled distribution policy precede accepted evidence',()=>{
  const f=fixture(),result=finalizeMacosSigning(f.options);
  assert.equal(result.notarizationStatus,'accepted-stapled');
  assert.equal(result.notarizationEvidence.application.id,appId);
  assert.equal(result.notarizationEvidence.dmg.id,dmgId);
  assert.ok(f.calls.some(args=>args[0] === 'syspolicy_check'));
  const submit=f.calls.findIndex(args=>args[0] === 'xcrun' && args[2] === 'submit');
  const staple=f.calls.findIndex(args=>args[0] === 'xcrun' && args[1] === 'stapler' && args[2] === 'staple');
  assert.ok(submit >= 0 && staple > submit);
});

test('rejected receipts wrong binary disabled Gatekeeper and stapling failure never downgrade',()=>{
  for (const failure of [{status:'Invalid'},{cdhash:'wrong'},{policy:'assessments disabled'},{failStaple:true}]) {
    const f=fixture(failure);assert.throws(()=>finalizeMacosSigning(f.options));
  }
  const f=fixture();f.options.packageResult.stdout='Skipping app notarization';
  assert.throws(()=>finalizeMacosSigning(f.options),/receipt/);
});

test('ticket service retries are bounded and never retry permanent stapling errors',()=>{
  const calls=[],delays=[];
  const transient=Object.assign(new Error('ticket service unavailable'),{exitCode:68});
  const run=(program,args)=>{calls.push([program,...args]);if(calls.length<3)throw transient;return {status:0};};
  assert.equal(runStapler({run,action:'staple',artifact:'verified.dmg',env:{},pause:value=>delays.push(value)}).status,0);
  assert.equal(calls.length,3);assert.deepEqual(delays,[20_000,20_000]);
  assert.ok(calls.every(args=>args.join(' ')==='xcrun stapler staple verified.dmg'));
  for(const exitCode of [65,66,73,77]) {
    let attempts=0;
    assert.throws(()=>runStapler({run:()=>{attempts++;throw Object.assign(new Error('permanent'),{exitCode});},action:'validate',artifact:'verified.dmg',env:{},pause:()=>assert.fail('must not wait')}),/permanent/);
    assert.equal(attempts,1);
  }
  let attempts=0;
  assert.throws(()=>runStapler({run:()=>{attempts++;throw transient;},action:'staple',artifact:'verified.dmg',env:{},pause:()=>{}}),/ticket service unavailable/);
  assert.equal(attempts,3);
});
