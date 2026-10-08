/** GPUI profile 的纯解析门禁；不使用 Tauri 插件合同，不推断缺失配置。 */
const fields = ['system_tray', 'system_notification', 'autostart', 'about_page', 'sponsor_page', 'single_instance', 'deep_link', 'global_shortcut', 'sidebar_mode'];

/** 命名代码块恰好一个且闭合，拒绝重复块或尾随未解析数据。 */
function block(markdown, name, required) {
  const headers = [...markdown.matchAll(new RegExp('^```' + name + '[ \\t]*\\r?$', 'gm'))];
  if (!headers.length && !required) return null;
  if (headers.length !== 1) throw new Error(`expected exactly one ${name} block`);
  const start = headers[0].index + headers[0][0].length;
  const tail = markdown.slice(start);
  const end = /^```[ \t]*\r?$/m.exec(tail);
  if (!end || tail.slice(0, end.index).includes('```')) throw new Error(`unclosed ${name} block`);
  return tail.slice(0, end.index).trim();
}

/** 九字段顺序固定；深链接保持不可用，快捷键中性合同必须独立存在或缺席。 */
export function parseGpuiInitializationProfile(markdown) {
  const lines = block(markdown, 'gui-initialization-config', true).split(/\r?\n/);
  if (lines.length !== fields.length) throw new Error('GPUI profile must have exactly nine fields');
  const result = {};
  for (const [index, line] of lines.entries()) {
    const match = /^\s*([a-z_]+)\s*:\s*(enabled|disabled|compact|detailed)\s*$/.exec(line);
    if (!match || match[1] !== fields[index]) throw new Error(`invalid or out-of-order GPUI field ${fields[index]}`);
    if (!(index === 8 ? ['compact', 'detailed'] : ['enabled', 'disabled']).includes(match[2])) throw new Error(`invalid GPUI value for ${match[1]}`);
    result[match[1]] = match[2];
  }
  if (result.deep_link !== 'disabled') throw new Error('GPUI deep_link is unavailable');
  const contract = block(markdown, 'gui-global-shortcut-contract', result.global_shortcut === 'enabled');
  if (result.global_shortcut === 'disabled' && contract !== null) throw new Error('disabled global_shortcut must not have a contract');
  if (contract !== null) {
    const parsed = JSON.parse(contract);
    if (parsed.schemaVersion !== 1 || !Array.isArray(parsed.actions) || parsed.actions.length !== 0 || Object.keys(parsed).sort().join(',') !== 'actions,schemaVersion') throw new Error('neutral GPUI shortcut contract must have schemaVersion 1 and empty actions');
  }
  return result;
}
