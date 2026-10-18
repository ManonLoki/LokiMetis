/** Linux Debian/便携归档的确定性布局和最终解包验证，不安装或启动应用。 */
import fs from 'node:fs';
import path from 'node:path';
import { safePath, sha256 } from './gpui_filesystem.mjs';
import { stageResources } from './gpui_resources.mjs';
import { verifyResources } from './gpui_packager.mjs';

/** 直接核对ELF64小端头和机器编号，交叉目标无需执行即可拒绝错误架构。 */
export function verifyElf(file, info) {
  const descriptor = fs.openSync(file, 'r'), header = Buffer.alloc(64);
  try { if (fs.readSync(descriptor, header, 0, 64, 0) !== 64) throw new Error('truncated ELF'); }
  finally { fs.closeSync(descriptor); }
  if (!header.subarray(0, 4).equals(Buffer.from([127,69,76,70])) || header[4] !== 2 || header[5] !== 1
      || header.readUInt16LE(18) !== info.machine) throw new Error('Linux executable ELF architecture mismatch');
  return 'verified-elf64';
}

/** .desktop与控制文件中不接受换行、占位展开或额外执行参数注入。 */
export function desktopEntry(config, command) {
  if (!/^\/[A-Za-z0-9/._-]+$/u.test(command)) throw new Error('unsafe desktop executable');
  return `[Desktop Entry]\nType=Application\nName=${config.productNameEn}\nName[zh_CN]=${config.productName}\nExec=${command}\nIcon=${config.identifier}\nStartupWMClass=${config.identifier}\nTerminal=false\nCategories=Utility;Development;\nStartupNotify=true\n`;
}

/** 依据真实ELF符号需求写glibc下界，不把新Ubuntu构建伪装为兼容旧发行版。 */
export function glibcRequirement(run, binary) {
  const versions = [...run('readelf', ['--version-info', binary]).stdout.matchAll(/\bGLIBC_(\d+)\.(\d+)\b/gu)]
    .map(match => [Number(match[1]), Number(match[2])]).sort((a,b) => a[0]-b[0] || a[1]-b[1]);
  if (!versions.length) throw new Error('Linux GNU executable has no verifiable GLIBC requirement');
  return versions.at(-1).join('.');
}

/** 发行树权限不依赖开发者umask；仅固定执行入口可执行，其余资源作为普通可读数据。 */
function normalizeLinuxModes(root, executables) {
  fs.chmodSync(root, 0o755);
  function walk(relative) {
    for (const name of fs.readdirSync(relative ? safePath(root,relative,{type:'directory'}) : root)) {
      const item=relative?`${relative}/${name}`:name, full=safePath(root,item), stat=fs.lstatSync(full);
      if (stat.isDirectory()) { fs.chmodSync(full,0o755); walk(item); }
      else if (stat.isFile()) fs.chmodSync(full,executables.has(item)?0o755:0o644);
      else throw new Error('Linux staging tree contains a special file');
    }
  }
  walk('');
}

/** 构建包内固定目录：主程序与resources相邻，与现有皮肤资源定位合同一致。 */
export function stageLinuxTree({root, work, config, facts, info, binary, resources, run, format}) {
  verifyElf(binary, info);
  const image = path.join(work, 'image'); fs.mkdirSync(image);
  const applicationRelative = format === 'deb' ? `opt/${config.identifier}` : `${config.productNameEn}`;
  const application = safePath(image, applicationRelative, {missing:true});
  fs.mkdirSync(path.dirname(application), {recursive:true});
  stageResources(root, application, resources);
  const copied = path.join(application, config.binary);
  fs.copyFileSync(binary, copied, fs.constants.COPYFILE_EXCL); fs.chmodSync(copied, 0o755);
  fs.copyFileSync(safePath(root, config.icons.source, {type:'file'}), path.join(application, 'app.png'), fs.constants.COPYFILE_EXCL);
  if (format === 'deb') {
    const wrapper = `usr/bin/${facts.project.replaceAll('_','-')}`;
    const destination = safePath(image, wrapper, {missing:true}); fs.mkdirSync(path.dirname(destination), {recursive:true});
    fs.writeFileSync(destination, `#!/bin/sh\nexec /opt/${config.identifier}/${config.binary} "$@"\n`, {mode:0o755, flag:'wx'});
    const desktop = path.join(image, 'usr/share/applications'); fs.mkdirSync(desktop, {recursive:true});
    fs.writeFileSync(path.join(desktop, `${config.identifier}.desktop`), desktopEntry(config, `/${wrapper}`), {flag:'wx'});
    const icons = path.join(image,'usr/share/icons/hicolor/1024x1024/apps'); fs.mkdirSync(icons, {recursive:true});
    fs.copyFileSync(path.join(application,'app.png'), path.join(icons, `${config.identifier}.png`));
    const control = path.join(image, 'DEBIAN'); fs.mkdirSync(control);
    const glibc = glibcRequirement(run, binary);
    const dependencies = [`libc6 (>= ${glibc})`, 'libgcc-s1', 'libstdc++6', 'libfontconfig1', 'libfreetype6',
      'libxkbcommon0', 'libxkbcommon-x11-0', 'libwayland-client0', 'libxcb1', 'libx11-6', 'libx11-xcb1', 'libvulkan1'];
    fs.writeFileSync(path.join(control,'control'), `Package: ${facts.project.replaceAll('_','-')}\nVersion: ${facts.version}\nArchitecture: ${info.debArch}\nMaintainer: ${config.publisher}\nSection: utils\nPriority: optional\nDepends: ${dependencies.join(', ')}\nDescription: ${config.productNameEn} native GPUI desktop application\n`, {flag:'wx'});
  } else {
    fs.writeFileSync(path.join(application,'launch'), `#!/bin/sh\napplication_dir=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd) || exit 1\nexec "$application_dir/${config.binary}" "$@"\n`, {mode:0o755, flag:'wx'});
  }
  normalizeLinuxModes(image,new Set([`${applicationRelative}/${config.binary}`,
    format==='deb'?`usr/bin/${facts.project.replaceAll('_','-')}`:`${applicationRelative}/launch`]));
  return {image, applicationRelative, binarySha256:sha256(copied)};
}

/** 最终归档必须与暂存树拥有相同文件、权限和摘要，拒绝额外文件及链接。 */
export function treeSnapshot(root) {
  const result = [];
  function walk(relative) {
    for (const name of fs.readdirSync(relative ? safePath(root, relative, {type:'directory'}) : root).sort()) {
      const item = relative ? `${relative}/${name}` : name, full = safePath(root,item), stat=fs.lstatSync(full);
      if (stat.isDirectory()) { result.push({path:item,type:'directory',mode:stat.mode & 0o777}); walk(item); }
      else if (stat.isFile()) result.push({path:item,type:'file',mode:stat.mode & 0o777,sha256:sha256(full)});
      else throw new Error('Linux package contains a special file');
    }
  }
  walk(''); return result;
}

/** 从本次生成的最终包解出真实内容，核对架构、应用、许可证、皮肤和桌面入口。 */
export function makeLinuxPackage({root, work, config, facts, info, binary, resources, run, format}) {
  const staged = stageLinuxTree({root,work,config,facts,info,binary,resources,run,format});
  const filename = `${facts.project.replaceAll('_','-')}-v${facts.version}-linux-${info.arch}.${format}`;
  const artifact = path.join(work, filename), extracted=path.join(work,'extracted');
  const expected = treeSnapshot(staged.image).filter(item => item.path!=='DEBIAN' && !item.path.startsWith('DEBIAN/'));
  if (format === 'deb') {
    run('dpkg-deb', ['--root-owner-group','--build',staged.image,artifact]);
    const fields=run('dpkg-deb',['--field',artifact,'Package','Version','Architecture']).stdout;
    for (const expected of [`Package: ${facts.project.replaceAll('_','-')}`,`Version: ${facts.version}`,`Architecture: ${info.debArch}`]) {
      if (!fields.split(/\r?\n/u).includes(expected)) throw new Error('Debian control identity mismatch');
    }
    run('dpkg-deb',['--extract',artifact,extracted]);
    const control = path.join(work,'extracted-control'); run('dpkg-deb',['--control',artifact,control]);
    if (fs.readFileSync(path.join(control,'control'),'utf8') !== fs.readFileSync(path.join(staged.image,'DEBIAN/control'),'utf8')) throw new Error('Debian control bytes differ');
    if (fs.readdirSync(control).join(',') !== 'control') throw new Error('unexpected Debian maintainer scripts');
  } else {
    const env={...process.env}; delete env.TAR_OPTIONS;
    run('tar',['--sort=name','--owner=0','--group=0','--numeric-owner','-czf',artifact,'-C',staged.image,'--',staged.applicationRelative],{env});
    fs.mkdirSync(extracted); run('tar',['--no-same-owner','--same-permissions','-xzf',artifact,'-C',extracted],{env});
  }
  if (JSON.stringify(treeSnapshot(extracted)) !== JSON.stringify(expected)) throw new Error('final Linux package tree differs');
  const app=safePath(extracted,staged.applicationRelative,{type:'directory'});
  verifyElf(safePath(app,config.binary,{type:'file'}),info);
  verifyResources(app,root,resources);
  if (sha256(path.join(app,config.binary)) !== staged.binarySha256) throw new Error('final Linux executable differs');
  return {artifact, filename, binarySha256:staged.binarySha256, resourceVerification:'final-linux-package-byte-identical'};
}
