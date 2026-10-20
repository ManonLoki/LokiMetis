/** 可选的本地网络声明只描述已批准的Bonjour服务，不申请签名或额外系统能力。 */
export function validateLocalNetwork(value) {
  if (value === undefined) return;
  if (!value || typeof value !== 'object' || Array.isArray(value)
      || Object.keys(value).sort().join(',') !== 'bonjourServices,usageDescription') throw new Error('invalid macos.localNetwork fields');
  if (typeof value.usageDescription !== 'string' || !value.usageDescription.trim()
      || value.usageDescription.length > 200 || /[\x00-\x1f\x7f]/u.test(value.usageDescription)
      || !value.usageDescription.isWellFormed()) throw new Error('invalid local network usage description');
  if (!Array.isArray(value.bonjourServices) || !value.bonjourServices.length || value.bonjourServices.length > 8
      || new Set(value.bonjourServices).size !== value.bonjourServices.length
      || value.bonjourServices.some(service => typeof service !== 'string' || !/^_[a-z0-9-]{1,15}\._(?:tcp|udp)$/u.test(service))) throw new Error('invalid Bonjour services');
}

/** 所有外部配置文字以XML文本转义，不允许插入其它plist键。 */
function xml(value) {
  return value.replaceAll('&', '&amp;').replaceAll('<', '&lt;').replaceAll('>', '&gt;').replaceAll('"', '&quot;').replaceAll("'", '&apos;');
}

/** 预览与正式包消费同一配置片段；未启用时不额外添加隐私声明。 */
export function renderLocalNetworkPlist(value) {
  validateLocalNetwork(value);
  if (value === undefined) return '';
  return `<key>NSLocalNetworkUsageDescription</key><string>${xml(value.usageDescription)}</string>\n<key>NSBonjourServices</key><array>${value.bonjourServices.map(service => `<string>${xml(service)}</string>`).join('')}</array>\n`;
}
