/** 显式平台映射展开为稳定文件清单，安装包不依赖构建机器源码目录。 */
import fs from 'node:fs';
import path from 'node:path';
import { isDeepStrictEqual } from 'node:util';
import { safePath, relativePath, sha256 } from './gpui_filesystem.mjs';

/** 所有目标按大小写不敏感规则检查文件冲突，避免跨平台覆盖。 */
function assertTargets(files, reserved) {
  const used = new Set(reserved.map(value => value.normalize('NFC').toLowerCase()));
  for (const file of files) {
    const key = relativePath(file.target).normalize('NFC').toLowerCase();
    for (const previous of used) {
      if (key === previous || key.startsWith(`${previous}/`) || previous.startsWith(`${key}/`)) throw new Error(`resource target collision: ${file.target}`);
    }
    used.add(key);
  }
}

/** 逐层拒绝链接、特殊文件和空映射，摘要同时覆盖被Git忽略的产品资源。 */
export function collectResources(root, config, platform, candidate) {
  if (!['macos','windows'].includes(platform)) throw new Error('unsupported resource platform');
  const mandatory = ['LICENSE.zh-CN.md','LICENSE.en.md',...(candidate ? ['release-notes.json'] : [])];
  const files = mandatory.map(source => ({source,target:source,sha256:sha256(safePath(root,source,{type:'file'}))}));
  const custom = [];
  function walk(source, target) {
    if (source.includes('${') || target.includes('${')) throw new Error('resource paths must not contain interpolation');
    const full = safePath(root,source), stat = fs.lstatSync(full);
    if (stat.isDirectory()) {
      const children = fs.readdirSync(full).sort();
      if (!children.length) throw new Error(`empty resource directory: ${source}`);
      for (const child of children) walk(`${source}/${child}`,`${target}/${child}`);
    } else if (stat.isFile()) custom.push({source,target,sha256:sha256(full)});
    else throw new Error(`resource must be an ordinary file or directory: ${source}`);
  }
  for (const item of config.resources ?? []) walk(item.source,item[platform]);
  assertTargets(custom,['LICENSE.zh-CN.md','LICENSE.en.md','release-notes.json',...(platform==='windows'?[`${config.binary}.exe`,'uninstall.exe']:[path.posix.basename(config.icons.macos)])]);
  files.push(...custom);
  return files.sort((a,b)=>a.target < b.target ? -1 : a.target > b.target ? 1 : 0);
}

/** 构建边界复算目录集合及字节，新增、删除和忽略文件漂移同样阻断。 */
export function assertResourcesStable(root, config, platform, candidate, expected) {
  if (!isDeepStrictEqual(collectResources(root,config,platform,candidate),expected)) throw new Error('packaging resources changed during build');
}

/** 暂存使用明确目标路径；复制前后核对捕获摘要，禁止接收竞态字节。 */
export function stageResources(root, destination, files) {
  fs.mkdirSync(destination);
  for (const file of files) {
    const source = safePath(root,file.source,{type:'file'});
    if (sha256(source)!==file.sha256) throw new Error(`resource source bytes changed: ${file.source}`);
    const target = safePath(destination,file.target,{missing:true});
    fs.mkdirSync(path.dirname(target),{recursive:true});
    fs.copyFileSync(source,target,fs.constants.COPYFILE_EXCL);
    if (sha256(target)!==file.sha256) throw new Error(`staged resource bytes differ: ${file.target}`);
  }
}
