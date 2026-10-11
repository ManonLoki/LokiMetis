/** 固定 NSIS 模板生成安全关闭、独立快捷方式与精确自启清理，不执行安装器。 */
import fs from 'node:fs';
import path from 'node:path';
import { createHash } from 'node:crypto';
import { safeRoot, safePath, relativePath } from './gpui_filesystem.mjs';

const TEMPLATE = new URL('../assets/nsis/cargo-packager-0.11.8.nsi', import.meta.url);
export const TEMPLATE_SHA256 = '3ff5fde09ae24dfc031ab91f939b290f394e2c6d3549f2c43c37414716a47b7a';
const CHECK = [
  '!macro CheckIfAppIsRunning',
  '  nsis_tauri_utils::FindProcess "${MAINBINARYNAME}.exe"',
  '  Pop $R0',
  '  ${If} $R0 = 0',
  '    IfSilent stop_running_app 0',
  '    ${If} $PassiveMode != 1',
  '      ${If} $LANGUAGE = 2052',
  '        MessageBox MB_OK|MB_ICONEXCLAMATION "${PRODUCTNAME} 正在运行。请正常退出应用后重新运行安装程序。"',
  '      ${Else}',
  '        MessageBox MB_OK|MB_ICONEXCLAMATION "${PRODUCTNAME} is running. Quit the application normally, then run setup again."',
  '      ${EndIf}',
  '    ${EndIf}',
  '    stop_running_app:',
  '    SetErrorLevel 2',
  '    Quit',
  '  ${EndIf}',
  '!macroend',
].join('\n');
const CLEANUP = [
  '  ; Remove only the exact current-user command owned by this installation.',
  '  ClearErrors',
  '  ReadRegStr $R8 HKCU "Software\\Microsoft\\Windows\\CurrentVersion\\Run" "${IDENTIFIER}"',
  '  ${IfNot} ${Errors}',
  '    ${If} $R8 == "$\\"$INSTDIR\\${MAINBINARYNAME}.exe$\\" "',
  '      DeleteRegValue HKCU "Software\\Microsoft\\Windows\\CurrentVersion\\Run" "${IDENTIFIER}"',
  '    ${ElseIf} $R8 == "$\\"$INSTDIR\\${MAINBINARYNAME}.exe$\\""',
  '      DeleteRegValue HKCU "Software\\Microsoft\\Windows\\CurrentVersion\\Run" "${IDENTIFIER}"',
  '    ${EndIf}',
  '  ${EndIf}',
].join('\n');

/** 正式英文名作为专属链接文件名；拒绝模板语法和非法文件名，美元符号保持字面值。 */
function shortcutLiteral(name) {
  if (typeof name !== 'string' || !name || name !== name.trim() || !name.isWellFormed() || name.length > 200
      || /[\\/:*?"<>|\x00-\x1f\x7f{}]/u.test(name) || name.endsWith('.')
      || /^(?:CON|PRN|AUX|NUL|COM[1-9]|LPT[1-9])(?:\.|$)/iu.test(name)) throw new Error('invalid NSIS shortcut name');
  return name.replaceAll('$', () => '$$');
}

/** 快捷方式创建、身份和删除共用正式英文名；开始菜单选择落盘且静默 /NS 零创建。 */
function isolateShortcuts(original, name) {
  const literal = shortcutLiteral(name);
  const identity = '!define PRODUCTNAME "{{product_name}}"\n';
  const page = '!insertmacro MUI_PAGE_STARTMENU Application $AppStartMenuFolder\n';
  const links = '${PRODUCTNAME}.lnk';
  const block = /  ; Create start menu shortcut \(GUI\)\n[\s\S]*?  shortcuts_done:\n/gu;
  if (original.split(identity).length !== 2 || original.split(page).length !== 2
      || original.split(links).length !== 7 || [...original.matchAll(block)].length !== 1) throw new Error('NSIS shortcut anchors differ');
  const settings = [
    '!define MUI_STARTMENUPAGE_DEFAULTFOLDER "${SHORTCUTNAME}"',
    '!define MUI_STARTMENUPAGE_REGISTRY_ROOT SHCTX',
    '!define MUI_STARTMENUPAGE_REGISTRY_KEY "${MANUPRODUCTKEY}"',
    '!define MUI_STARTMENUPAGE_REGISTRY_VALUENAME "Start Menu Folder"',
  ].join('\n');
  const create = [
    '  ; Silent/passive /NS bypasses every shortcut creation path.',
    '  IfSilent check_ns_flag 0',
    '  ${IfThen} $PassiveMode == 1 ${|} Goto check_ns_flag ${|}',
    '  Goto create_start_menu',
    '  check_ns_flag:',
    '    ClearErrors',
    '    ${GetOptions} $CMDLINE "/NS" $R0',
    '    IfErrors 0 shortcuts_done',
    '    Call CreateDesktopShortcut',
    '  create_start_menu:',
    '  !insertmacro MUI_STARTMENU_WRITE_BEGIN Application',
    '    Call CreateStartMenuShortcut',
    '  !insertmacro MUI_STARTMENU_WRITE_END',
    '  shortcuts_done:',
    '',
  ].join('\n');
  return original.replace(identity, () => `${identity}!define SHORTCUTNAME "${literal}"\n`)
    .replaceAll(links, '${SHORTCUTNAME}.lnk').replace(page, `${settings}\n${page}`).replace(block, create);
}

/** 上游只列直接父目录且按字典序删除；从实际文件目标补齐祖先，普通 RMDir 保留用户文件。 */
function resourceCleanup(original, targets) {
  const anchor = '  {{#each resources_dirs}}\n  RMDir /REBOOTOK "$INSTDIR\\\\{{this}}"\n  {{/each}}\n';
  if (original.split(anchor).length !== 2) throw new Error('NSIS resource cleanup anchor differs');
  if (!Array.isArray(targets)) throw new Error('invalid NSIS resource targets');
  const directories = new Map();
  for (const target of targets) {
    relativePath(target);
    if (!target.isWellFormed() || /["{}]/u.test(target)) throw new Error('invalid NSIS resource target');
    const parts = target.split('/'); parts.pop();
    while (parts.length) {
      const directory = parts.join('/'), key = directory.normalize('NFC').toLowerCase();
      if (!directories.has(key)) directories.set(key, directory);
      parts.pop();
    }
  }
  const ordered = [...directories.values()].sort((left, right) => right.split('/').length - left.split('/').length || (left < right ? -1 : left > right ? 1 : 0));
  const commands = ordered.map(directory => `  RMDir "$INSTDIR\\${directory.replaceAll('$', () => '$$').replaceAll('/', '\\')}"\n`).join('');
  return original.replace(anchor, () => commands);
}

/** 摘要及唯一锚点任一漂移均零输出；保留原始模板其余全部安装行为。 */
export function renderNsisTemplate(shortcutName, resourceTargets = [], bytes = fs.readFileSync(TEMPLATE)) {
  if (createHash('sha256').update(bytes).digest('hex') !== TEMPLATE_SHA256) throw new Error('fixed NSIS template digest differs');
  const original = resourceCleanup(isolateShortcuts(bytes.toString('utf8'), shortcutName), resourceTargets);
  const matches = [...original.matchAll(/!macro CheckIfAppIsRunning\n[\s\S]*?\n!macroend/gu)];
  if (matches.length !== 1) throw new Error('NSIS process macro anchor differs');
  const anchor = 'Section Uninstall\n  !insertmacro CheckIfAppIsRunning\n';
  if (original.split(anchor).length !== 2) throw new Error('NSIS uninstall anchor differs');
  const initialize = 'Function un.onInit\n';
  if (original.split(initialize).length !== 2) throw new Error('NSIS uninstall initialization anchor differs');
  const passive = '  StrCpy $PassiveMode 0\n  ClearErrors\n  ${GetOptions} $CMDLINE "/P" $R0\n  ${IfNot} ${Errors}\n    StrCpy $PassiveMode 1\n  ${EndIf}\n';
  const reinstall = 'Function PageLeaveReinstall\n';
  const invokeOld = '  reinst_uninstall:\n';
  if (original.split(reinstall).length !== 2 || original.split(invokeOld).length !== 2) throw new Error('NSIS upgrade anchor differs');
  const declaration = 'Var PassiveMode\n';
  if (original.split(declaration).length !== 2) throw new Error('NSIS passive variable anchor differs');
  return original.replace(matches[0][0], '').replace(declaration, '').replace(reinstall, `${declaration}${CHECK}\n\n${reinstall}`)
    .replace(invokeOld, `${invokeOld}    !insertmacro CheckIfAppIsRunning\n`)
    .replace(anchor, `${anchor}\n${CLEANUP}\n`).replace(initialize, initialize + passive);
}

/** 本次暂存目录内创建唯一普通模板，不接受项目提供的任意安装脚本。 */
export function writeNsisTemplate(work, shortcutName, resourceTargets = []) {
  safeRoot(work);
  const destination = safePath(work, 'installer.nsi', { missing: true });
  fs.writeFileSync(destination, renderNsisTemplate(shortcutName, resourceTargets), { flag: 'wx' });
  if (path.dirname(destination) !== path.resolve(work)) throw new Error('NSIS template escaped staging directory');
  return destination;
}
