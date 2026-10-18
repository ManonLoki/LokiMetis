/** 用隔离命令桩验证身份与 argv；从不运行真实 apt、sudo、rustup 或修改用户环境。 */
import assert from 'node:assert/strict';
import fs from 'node:fs';
import path from 'node:path';
import os from 'node:os';
import {spawnSync} from 'node:child_process';
import {fileURLToPath} from 'node:url';
import test from 'node:test';

const script=fileURLToPath(new URL('./linux-gpui-dependencies.sh',import.meta.url));
const posixOnly={skip:process.platform==='win32'};

/** 每个外部命令都由同一隔离Node夹具记录，原始HOME只透传且不会写入。 */
function fixture(t) {
  const root=fs.mkdtempSync(path.join(fs.realpathSync(os.tmpdir()),'gpui-linux-deps-'));
  t.after(()=>fs.rmSync(root,{recursive:true,force:true}));
  const bin=path.join(root,'bin'), log=path.join(root,'calls.jsonl'); fs.mkdirSync(bin);
  const body=`#!${process.execPath}
import fs from 'node:fs';
import path from 'node:path';
import {spawnSync} from 'node:child_process';
const tool=path.basename(process.argv[1]),args=process.argv.slice(2);
fs.appendFileSync(process.env.FIXTURE_LOG,JSON.stringify({tool,args,home:process.env.HOME})+'\\n');
if(tool==='uname') process.stdout.write('Linux\\n');
else if(tool==='id') process.stdout.write(process.env.FIXTURE_UID+'\\n');
else if(tool==='dpkg') {
  if(args[0]==='--print-architecture') process.stdout.write('amd64\\n');
  else if(args[0]!=='--print-foreign-architectures') throw Error('unexpected dpkg mutation');
} else if(tool==='sudo') {
  if(args[0]!=='--'||args[1]!=='apt-get') throw Error('unexpected privilege argv');
  const result=spawnSync(path.join(path.dirname(process.argv[1]),'apt-get'),args.slice(2),{stdio:'inherit',env:process.env});
  process.exit(result.status??99);
} else if(tool==='rustup') {
  if(args.join(' ')==='target list --installed') process.stdout.write('x86_64-unknown-linux-gnu\\n');
  else if(args.join(' ')!=='target add x86_64-unknown-linux-gnu') throw Error('unexpected rustup argv');
} else if(tool==='pkg-config') process.stdout.write('1.0\\n');
else if(tool==='awk') {
  const expected=args.find(value=>value.startsWith('expected='))?.slice(9);
  process.exit(fs.readFileSync(0,'utf8').trim().split(/\\r?\\n/).includes(expected)?0:1);
} else if(!['apt-get','cargo','rustc','x86_64-linux-gnu-gcc','x86_64-linux-gnu-g++','readelf','tar','dpkg-deb','dbus-daemon'].includes(tool)) throw Error('unexpected external tool');
`;
  for(const tool of ['uname','id','dpkg','apt-get','sudo','rustup','cargo','rustc','x86_64-linux-gnu-gcc','x86_64-linux-gnu-g++','pkg-config','readelf','tar','dpkg-deb','dbus-daemon','awk']) {
    const target=path.join(bin,tool); fs.writeFileSync(target,body); fs.chmodSync(target,0o755);
  }
  return {bin,log};
}

/** shell只见隔离PATH；目标与伪原生架构相同，不可能进入绝对dpkg写入路径。 */
function execute(t,uid,mode='--install',identity={}) {
  const {bin,log}=fixture(t);
  const result=spawnSync('/bin/sh',[script,mode,'--target','x86_64-unknown-linux-gnu'],{
    env:{...process.env,...identity,PATH:bin,FIXTURE_UID:String(uid),FIXTURE_LOG:log},encoding:'utf8',timeout:10_000,
  });
  assert.ifError(result.error);
  const calls=fs.readFileSync(log,'utf8').trim().split('\n').filter(Boolean).map(line=>JSON.parse(line));
  assert.ok(calls.every(call=>call.home===process.env.HOME),'script must not guess or redirect HOME');
  return {...result,calls};
}

for(const identity of [{},{SUDO_UID:'1000',SUDO_USER:'owner'},{PKEXEC_UID:'1000'}]) {
  test(`root admin stage leaves Rust setup to original user: ${JSON.stringify(identity)}`,posixOnly,t=>{
    const result=execute(t,0,'--install',identity);
    assert.equal(result.status,20,result.stderr);
    assert.match(result.stdout,/linux\.status=pending-original-user/u);
    assert.match(result.stdout,/rustup target add x86_64-unknown-linux-gnu/u);
    assert.deepEqual(result.calls.filter(call=>call.tool==='rustup'||call.tool==='cargo'||call.tool==='rustc'),[]);
    assert.deepEqual(result.calls.filter(call=>call.tool==='sudo'),[]);
    const apt=result.calls.filter(call=>call.tool==='apt-get').map(call=>call.args);
    assert.deepEqual(apt[0],['update']);
    assert.equal(apt[1][0],'install'); assert.equal(apt[1][1],'-y');
    assert.ok(apt[1].includes('dbus-daemon'));
    assert.ok(apt[1].includes('libfontconfig1-dev:amd64'));
  });
}

test('nonroot install elevates only apt argv and installs Rust target in its own account',posixOnly,t=>{
  const result=execute(t,1000);
  assert.equal(result.status,0,result.stderr);
  assert.deepEqual(result.calls.filter(call=>call.tool==='rustup').map(call=>call.args),[
    ['target','add','x86_64-unknown-linux-gnu'],['target','list','--installed'],
  ]);
  const elevated=result.calls.filter(call=>call.tool==='sudo').map(call=>call.args);
  assert.equal(elevated.length,2);
  assert.deepEqual(elevated[0],['--','apt-get','update']);
  assert.ok(elevated.every(args=>args[0]==='--'&&args[1]==='apt-get'));
  assert.doesNotMatch(result.stdout,/pending-original-user/u);
});

test('root readonly checks do not mistake the administrator Rust account for user readiness',posixOnly,t=>{
  const result=execute(t,0,'--check-only');
  assert.equal(result.status,20,result.stderr);
  assert.match(result.stdout,/linux\.status=pending-original-user/u);
  assert.deepEqual(result.calls.filter(call=>['apt-get','sudo','rustup','cargo','rustc'].includes(call.tool)),[]);
});

test('nonroot readonly checks never install or elevate',posixOnly,t=>{
  const result=execute(t,1000,'--check-only');
  assert.equal(result.status,0,result.stderr);
  assert.deepEqual(result.calls.filter(call=>['apt-get','sudo'].includes(call.tool)),[]);
  assert.deepEqual(result.calls.filter(call=>call.tool==='rustup').map(call=>call.args),[['target','list','--installed']]);
  assert.match(result.stdout,/linux\.tool\.dbus-daemon=present/u);
});
