/** Debian应用目录使用标准DEP-11 YAML，供应用中心关联已安装包、桌面入口与主题图标。 */

/** 为描述段落转义XML文本，防止产品文案被解释为AppStream标记。 */
function markupText(value) {
  return value.replaceAll('&','&amp;').replaceAll('<','&lt;').replaceAll('>','&gt;');
}

/** 仅从既有打包身份生成目录；JSON字符串也是合法YAML标量，不拼入未转义的产品文案。 */
export function linuxAppstreamCatalog(config,facts) {
  const quote=JSON.stringify;
  const metadata=config.linux ?? {
    summary:'Native desktop application',summaryZh:'原生桌面应用',
    description:`Run ${config.productNameEn} as a native desktop application.`,
    descriptionZh:`以原生桌面应用运行${config.productName}。`,
  };
  return `---
File: DEP-11
Version: '0.16'
Origin: ${quote(config.identifier)}
---
Type: desktop-application
ID: ${quote(config.identifier)}
Package: ${quote(facts.project.replaceAll('_','-'))}
Name:
  C: ${quote(config.productNameEn)}
  zh_CN: ${quote(config.productName)}
Summary:
  C: ${quote(metadata.summary)}
  zh_CN: ${quote(metadata.summaryZh)}
Description:
  C: ${quote(`<p>${markupText(metadata.description)}</p>`)}
  zh_CN: ${quote(`<p>${markupText(metadata.descriptionZh)}</p>`)}
DeveloperName:
  C: ${quote(config.publisher)}
Icon:
  stock: ${quote(config.identifier)}
Launchable:
  desktop-id:
    - ${quote(`${config.identifier}.desktop`)}
Categories:
  - Utility
  - Development
`;
}
