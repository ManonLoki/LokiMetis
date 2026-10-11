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
  assert.throws(() => renderNsisTemplate('LokiMetris', Buffer.from('different template')), /digest differs/u);
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
