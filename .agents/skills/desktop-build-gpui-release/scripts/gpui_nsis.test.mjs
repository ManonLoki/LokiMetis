import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import assert from 'node:assert/strict';
import { test } from 'node:test';
import { renderNsisTemplate, writeNsisTemplate } from './gpui_nsis.mjs';

test('fixed template stops both installer modes without terminating any process', () => {
  const rendered = renderNsisTemplate('LokiMetris');
  assert.equal(rendered.includes('::KillProcess'), false);
  assert.equal(rendered.match(/!insertmacro CheckIfAppIsRunning/gu).length, 3);
  const upgrade = rendered.match(/Function PageLeaveReinstall\n[\s\S]*?\nFunctionEnd/u)[0];
  assert.match(upgrade, /reinst_uninstall:\n    !insertmacro CheckIfAppIsRunning\n    HideWindow/u);
  assert.ok(rendered.indexOf('!macro CheckIfAppIsRunning') < rendered.indexOf('Function PageLeaveReinstall'));
  assert.equal(rendered.match(/Var PassiveMode\n/gu).length, 1);
  assert.ok(rendered.indexOf('Var PassiveMode\n') < rendered.indexOf('!macro CheckIfAppIsRunning'));
  assert.ok(upgrade.indexOf('!insertmacro CheckIfAppIsRunning') < upgrade.indexOf('ExecWait'));
  const macro = rendered.match(/!macro CheckIfAppIsRunning\n[\s\S]*?\n!macroend/u)[0];
  assert.match(macro, /IfSilent stop_running_app 0/u);
  assert.match(macro, /\$PassiveMode != 1/u);
  assert.match(macro, /stop_running_app:\n    SetErrorLevel 2\n    Quit/u);
  assert.equal(macro.includes('Abort'), false);
  const uninit = rendered.match(/Function un.onInit\n[\s\S]*?\nFunctionEnd/u)[0];
  assert.match(uninit, /StrCpy \$PassiveMode 0/u);
  assert.match(uninit, /\$\{GetOptions\} \$CMDLINE "\/P" \$R0/u);
  assert.match(uninit, /\$\{IfNot\} \$\{Errors\}\n    StrCpy \$PassiveMode 1/u);
  assert.equal(macro.includes('Sleep'), false);
  assert.match(macro, /正常退出/u);
  assert.match(macro, /Quit the application normally/u);
});

test('uninstall only removes exact owned current-user Run commands before the executable', () => {
  const section = renderNsisTemplate('LokiMetris').split('Section Uninstall\n')[1];
  const end = section.indexOf('  ; Delete the app directory');
  const cleanup = section.slice(0, end);
  assert.match(cleanup, /ReadRegStr \$R8 HKCU/u);
  assert.match(cleanup, /\$\{IfNot\} \$\{Errors\}/u);
  assert.ok(cleanup.includes('${If} $R8 == "$\\"$INSTDIR\\${MAINBINARYNAME}.exe$\\" "'));
  assert.ok(cleanup.includes('${ElseIf} $R8 == "$\\"$INSTDIR\\${MAINBINARYNAME}.exe$\\""'));
  assert.equal(cleanup.match(/DeleteRegValue HKCU/gu).length, 2);
  assert.equal(cleanup.includes('DeleteRegKey'), false);
  assert.equal(cleanup.includes('HKLM'), false);
  assert.equal(cleanup.includes('StartupApproved'), false);
});

test('source drift fails closed and staging template cannot replace an existing file', t => {
  assert.throws(() => renderNsisTemplate('LokiMetris', [], Buffer.from('different template')), /digest differs/u);
  const temporary = fs.realpathSync(os.tmpdir());
  const work = fs.mkdtempSync(path.join(temporary, 'loki-nsis-test-'));
  t.after(() => {
    assert.equal(fs.realpathSync(work), work);
    assert.equal(path.dirname(work), temporary);
    assert.ok(path.basename(work).startsWith('loki-nsis-test-'));
    fs.rmSync(work, { recursive: true, force: true });
  });
  const output = writeNsisTemplate(work, 'LokiMetris');
  assert.equal(path.dirname(output), fs.realpathSync(work));
  assert.equal(fs.readFileSync(output, 'utf8'), renderNsisTemplate('LokiMetris'));
  assert.throws(() => writeNsisTemplate(work, 'LokiMetris'), /exist/u);
  assert.equal(fs.readFileSync(output, 'utf8'), renderNsisTemplate('LokiMetris'));
});

/** 安装与卸载不触及参考版中文链接，任意开始菜单选择由 MUI 同一注册表合同恢复。 */
test('official English identity owns all shortcut paths and custom start menu folder survives uninstall', () => {
  const rendered = renderNsisTemplate('LokiMetris');
  assert.match(rendered, /!define SHORTCUTNAME "LokiMetris"/u);
  assert.equal(rendered.includes('${PRODUCTNAME}.lnk'), false);
  assert.equal(rendered.match(/\$\{SHORTCUTNAME\}\.lnk/gu).length, 6);
  assert.match(rendered, /MUI_STARTMENUPAGE_DEFAULTFOLDER "\$\{SHORTCUTNAME\}"/u);
  assert.match(rendered, /MUI_STARTMENUPAGE_REGISTRY_ROOT SHCTX/u);
  assert.match(rendered, /MUI_STARTMENUPAGE_REGISTRY_KEY "\$\{MANUPRODUCTKEY\}"/u);
  assert.match(rendered, /MUI_STARTMENUPAGE_REGISTRY_VALUENAME "Start Menu Folder"/u);
  const create = rendered.slice(rendered.indexOf('  ; Silent/passive /NS'), rendered.indexOf('  shortcuts_done:'));
  assert.equal(create.match(/Call CreateStartMenuShortcut/gu).length, 1);
  assert.ok(create.indexOf('IfErrors 0 shortcuts_done') < create.indexOf('Call CreateDesktopShortcut'));
  assert.ok(create.indexOf('IfErrors 0 shortcuts_done') < create.indexOf('Call CreateStartMenuShortcut'));
  assert.match(create, /MUI_STARTMENU_WRITE_BEGIN Application\n    Call CreateStartMenuShortcut\n  !insertmacro MUI_STARTMENU_WRITE_END/u);
  assert.match(rendered, /MUI_STARTMENU_GETFOLDER Application \$AppStartMenuFolder/u);
});

/** 正式名作为字面 NSIS 文件名处理，非法路径、设备名和模板注入均失败关闭。 */
test('shortcut literals reject code and path injection while preserving dollars', () => {
  assert.match(renderNsisTemplate('Test $1 App'), /!define SHORTCUTNAME "Test \$\$1 App"/u);
  for (const value of [undefined, '', ' bad', 'bad.', 'a/b', 'a\\b', 'a"b', 'a\nb', '${INSTDIR}', '{{identifier}}', 'CON', 'LPT1.app']) {
    assert.throws(() => renderNsisTemplate(value), /invalid NSIS shortcut name/u);
  }
});

/** 只提取资源清理语句，验证生成的实际目录顺序和非递归语义。 */
function resourceDirectories(targets) {
  const uninstall = renderNsisTemplate('LokiMetris', targets).split('Section Uninstall\n')[1];
  return [...uninstall.matchAll(/^  RMDir "\$INSTDIR\\([^"\n]+)"$/gmu)].map(match => match[1].replaceAll('\\', '/'));
}

/** 安装的全部文件目标推导完整祖先，根许可证不生成空目录，父级晚于所有子级。 */
test('uninstall derives every resource ancestor once in child-first order', () => {
  const directories = resourceDirectories(['LICENSE.en.md', 'resources/skins/generator/assets/fonts/a.ttf',
    'resources/skins/generator/assets/fonts/b.ttf', 'resources/third-party/license.txt']);
  assert.deepEqual(new Set(directories), new Set(['resources', 'resources/skins', 'resources/skins/generator',
    'resources/skins/generator/assets', 'resources/skins/generator/assets/fonts', 'resources/third-party']));
  assert.equal(directories.length, 6);
  for (const [index, directory] of directories.entries()) {
    for (const child of directories.filter(value => value.startsWith(`${directory}/`))) assert.ok(index > directories.indexOf(child));
  }
  const rendered = renderNsisTemplate('LokiMetris', ['resources/a/b.txt']);
  assert.match(rendered, /CreateDirectory "\$INSTDIR\\\\\{\{this\}\}"/u);
  assert.equal(rendered.split('Section Uninstall\n')[1].includes('{{#each resources_dirs}}'), false);
});

/** 不允许模板、路径注入；NSIS 变量形状的目录必须保留为字面值。 */
test('resource cleanup rejects unsafe targets and preserves literal dollars', () => {
  assert.deepEqual(resourceDirectories(['resources/$0/ok.txt']), ['resources/$$0', 'resources']);
  for (const target of ['../escape/file', '/absolute/file', 'a\\b', 'a//b', 'a/CON/file', 'a/quote"/file', 'a/{{identifier}}/file', 'a/line\n/file']) {
    assert.throws(() => renderNsisTemplate('LokiMetris', [target]), /unsafe|invalid NSIS resource/u);
  }
});

/** 真实空树可以删除；新增用户文件后普通 RMDir 保留该目录及其祖先。 */
test('generated cleanup removes empty trees while retaining user files', t => {
  const temporary = fs.realpathSync(os.tmpdir()), root = fs.mkdtempSync(path.join(temporary, 'loki-nsis-test-'));
  t.after(() => {
    assert.equal(fs.realpathSync(root), root); assert.equal(path.dirname(root), temporary);
    assert.ok(path.basename(root).startsWith('loki-nsis-test-')); fs.rmSync(root, { recursive: true, force: true });
  });
  const targets = ['resources/skins/generator/assets/fonts/a.ttf', 'resources/third-party/license.txt'];
  for (const retainUserFile of [false, true]) {
    const installation = path.join(root, retainUserFile ? 'user-files' : 'empty');
    for (const target of targets) { const file = path.join(installation, target); fs.mkdirSync(path.dirname(file), { recursive: true }); fs.writeFileSync(file, 'owned'); }
    const user = path.join(installation, 'resources/skins/generator/assets/user-note.txt');
    if (retainUserFile) fs.writeFileSync(user, 'preserve');
    for (const target of targets) fs.unlinkSync(path.join(installation, target));
    for (const directory of [...resourceDirectories(targets), '']) {
      try { fs.rmdirSync(path.join(installation, directory)); } catch (error) { assert.ok(retainUserFile && error.code === 'ENOTEMPTY'); }
    }
    assert.equal(fs.existsSync(installation), retainUserFile);
    if (retainUserFile) assert.equal(fs.readFileSync(user, 'utf8'), 'preserve');
  }
});
