#!/usr/bin/env node
import { readFile, writeFile } from "node:fs/promises";
import path from "node:path";
import { SCRIPT_DIR, SURFACES, dataUrl, ensureDirectory, escapeHtml, exists, parseArgs, readJson, requireValue } from "./lib.mjs";
import { parseModeThemeCss, parseThemeCss } from "./theme_css.mjs";

const escape = escapeHtml;

async function themeRuntimeDirectory() {
  for (const candidate of [
    path.resolve(SCRIPT_DIR, "../../theme-runtime"),
    path.resolve(SCRIPT_DIR, "../../../resources/theme-runtime"),
    path.resolve(SCRIPT_DIR, "../../../../resources/theme-runtime"),
    path.resolve(SCRIPT_DIR, "../../../../loki_metis_gui/resources/theme-runtime"),
  ]) {
    if (await exists(path.join(candidate, "theme.css"))) return candidate;
  }
  throw new Error("无法定位 LokiMetis 统一主题运行时资源。");
}

function panelsMarkup(manifest, assets) {
  if ([2, 3].includes(manifest.schemaVersion) && manifest.type === "theme") {
    return "";
  }
  const panels = Array.isArray(manifest.extensions?.photoPanels) ? manifest.extensions.photoPanels : [];
  return panels.filter((panel) => panel && panel.enabled).slice(0, 2).map((panel) => {
    const source = assets[panel.assetSlot];
    if (!source) return "";
    const offset = panel.offset && typeof panel.offset === "object" ? panel.offset : {};
    const size = panel.size && typeof panel.size === "object" ? panel.size : {};
    const visible = Array.isArray(panel.visibleOn) ? panel.visibleOn : ["chat"];
    const caption = panel.title || panel.caption ? `<figcaption>${panel.title ? `<b>${escape(panel.title)}</b>` : ""}${panel.caption ? `<span>${escape(panel.caption)}</span>` : ""}</figcaption>` : "";
    return `<figure class="skin-photo-panel" data-panel-id="${escape(panel.id)}" data-anchor="${escape(panel.anchor ?? "left-top")}" data-fit="${escape(panel.fit ?? "contain")}" data-visible-on="${escape(visible.join(" "))}" data-min-viewport-width="${escape(panel.minViewportWidth ?? 1280)}" style="--panel-x:${escape(offset.x ?? 24)}px;--panel-y:${escape(offset.y ?? 24)}px;width:${escape(size.width ?? 304)}px;height:${escape(size.height ?? 360)}px"><img src="${source}" alt="">${caption}</figure>`;
  }).join("");
}

function surfaceMarkup(surface, preview, assets, hidden) {
  const title = (field, fallback) => escape(typeof preview[field] === "string" && preview[field].trim() ? preview[field] : fallback);
  const hide = hidden(surface) ? " hidden" : "";
  const content = {
    home: `<span data-testid="home-icon" hidden></span><div data-feature="game-source"><h1>${escape(preview.homeTitle)}</h1><p>${escape(preview.homeSubtitle)}</p></div><div class="group/home-suggestions" role="list"><button role="listitem">${escape(preview.suggestionOne)}</button><button role="listitem">${escape(preview.suggestionTwo)}</button></div>`,
    chat: `<div class="thread-scroll-container"><article data-virtualized-turn-content data-message-author-role="user">${escape(preview.userMessage)}</article><article data-virtualized-turn-content data-message-author-role="assistant">${escape(preview.assistantMessage)}<pre><code>pnpm check</code></pre></article></div>`,
    "pull-requests": `<div class="preview-feature-header"><h1>${title("pullRequestsTitle", "拉取请求")}</h1><input id="pull-request-inbox-search" aria-label="搜索拉取请求" placeholder="搜索"></div><div class="preview-split preview-pr-layout"><div role="list"><div role="status" class="preview-empty"><b>${title("emptyStateTitle", "暂无内容")}</b><span>GitHub CLI 尚未就绪</span><button>${title("emptyStateAction", "重新检查")}</button></div></div><div role="separator" aria-orientation="vertical"></div><div class="preview-detail"><div role="tablist" aria-label="拉取请求详情"><button id="preview-pull-request-summary-tab" role="tab" aria-selected="true">摘要</button><button id="preview-pull-request-code-tab" role="tab" aria-selected="false">代码</button><button id="preview-pull-request-activity-tab" role="tab" aria-selected="false">活动</button></div><div id="preview-pull-request-summary-panel" role="tabpanel" aria-labelledby="preview-pull-request-summary-tab"><div class="preview-empty"><span>选择拉取请求以查看详情</span></div></div></div></div>`,
    sites: `<div class="preview-feature-header"><div><h1>${title("sitesTitle", "站点")}</h1><p>管理由会话创建的站点</p></div><input id="appgen-site-search" placeholder="搜索站点"><button>创建站点</button></div><div class="preview-card-grid" role="list"><article role="listitem"><img data-testid="library-file-thumbnail" src="${assets.art}" alt=""><h2>项目预览</h2><p>私有 · 刚刚更新</p></article><article role="listitem"><div class="preview-thumbnail" aria-hidden="true"></div><h2>文档站点</h2><p>共享 · 昨天更新</p></article></div>`,
    automations: `<div class="preview-feature-header"><h1>${title("automationsTitle", "已安排")}</h1><button>新建安排</button></div><div class="preview-split"><div role="list"><div class="automation-row" role="listitem" aria-selected="true"><b>每日检查</b><span>每天 09:00</span></div><div class="automation-row" role="listitem"><b>每周总结</b><span>每周五 18:00</span></div></div><div role="separator" aria-orientation="vertical"></div><form aria-labelledby="automation-detail-panel-title"><input id="automation-detail-panel-title" value="每日检查" aria-label="安排标题"><label>重复<select><option>每天</option></select></label><label>说明<textarea>检查项目状态并汇总结果</textarea></label><label><button type="button" role="switch" data-state="checked" aria-checked="true">启用通知</button></label></form></div>`,
    plugins: `<div class="preview-feature-header"><h1>${title("pluginsTitle", "插件")}</h1></div><div class="sticky bg-token-main-surface-primary preview-plugin-search-chrome"><div><input id="plugins-page-search" aria-label="浏览插件或技能" placeholder="搜索插件"></div></div><div role="tablist" aria-label="插件目录"><button role="tab" aria-selected="true">已安装</button><button role="tab" aria-selected="false">市场</button></div><section id="plugins-search-installed"><h2>已安装</h2><div class="preview-card-grid" role="list"><article role="listitem"><div class="preview-plugin-icon">G</div><h2>GitHub</h2><p>仓库、议题和拉取请求</p><button role="switch" data-state="checked" aria-checked="true">已启用</button></article></div></section><section id="plugins-marketplace-featured"><h2>精选</h2><div class="preview-card-grid" role="list"><article role="listitem"><div class="preview-plugin-icon">S</div><h2>Slack</h2><p>搜索与协作消息</p><button>安装</button></article></div></section>`,
    settings: `<div data-app-shell-focus-area="main"><div><div class="preview-settings-surface"><div class="preview-feature-header"><h1>${title("settingsTitle", "设置")}</h1><input role="searchbox" placeholder="搜索设置"></div><div class="preview-settings-layout"><nav aria-label="设置"><button data-settings-panel-slug="general-settings" aria-current="page">通用</button><button data-settings-panel-slug="appearance">外观</button><button data-settings-panel-slug="plugins-settings">插件</button></nav><form class="preview-settings"><label>外观<select><option>跟随系统</option></select></label><section><h2>权限</h2><div class="preview-settings-card border-default"><div><span>默认权限</span><button type="button" role="switch" data-state="checked" aria-checked="true" aria-label="默认权限"><span data-state="checked"><span data-state="checked"></span></span></button></div><div><span>完整访问权限</span><button type="button" role="switch" data-state="unchecked" aria-checked="false" aria-label="完整访问权限"><span data-state="unchecked"><span data-state="unchecked"></span></span></button></div></div></section><label>项目目录<input value="/workspace/example" disabled></label><div role="alert">示例提示：设置会自动保存。</div></form></div></div></div></div>`,
    "other": `<div class="preview-empty"><b>${title("emptyStateTitle", "暂无内容")}</b><span>未分类页面使用通用语义样式。</span></div>`,
  }[surface];
  const className = surface === "home" ? "preview-surface preview-home" : surface === "chat" ? "preview-surface" : "preview-surface preview-feature";
  return `<section id="${surface}" class="${className}" role="main"${hide}>${content}</section>`;
}

export async function buildPreview(directory, initialSurface = "chat", initialMode = "dark") {
  const manifest = await readJson(path.join(directory, "theme.json"));
  const pureTheme = [2, 3].includes(manifest.schemaVersion) && manifest.type === "theme";
  let themeCss = manifest.schemaVersion === 3
    ? await readFile(path.join(directory, "theme.css"), "utf8")
    : "";
  const asset = async (file) => {
    if (!file) return "";
    const mime = /\.png$/i.test(file) ? "image/png" : "image/jpeg";
    return dataUrl(await readFile(path.join(directory, file)), mime);
  };
  const themeConfig = themeCss ? parseThemeCss(themeCss, { explicitAppearance: manifest.appearance !== undefined }) : null;
  const modeBackgrounds = {};
  for (const mode of manifest.appearance?.supportedColorModes ?? []) {
    const file = `theme.${mode}.css`;
    const overlay = await readFile(path.join(directory, file), "utf8");
    const parsed = parseModeThemeCss(overlay, mode, file);
    if (parsed.background) modeBackgrounds[mode] = await asset(parsed.background);
    themeCss += `\n${overlay}`;
  }
  const skinCssPath = pureTheme
    ? path.join(await themeRuntimeDirectory(), "theme.css")
    : path.join(directory, "dream-skin.css");
  const skinCss = `${await readFile(skinCssPath, "utf8")}\n${themeCss}`.replaceAll("</style", "<\\/style");
  const assets = pureTheme ? {
    art: await asset(themeConfig?.background ?? manifest.images?.background),
  } : {
    art: dataUrl(await readFile(path.join(directory, "qq2007-sky.png")), "image/png"),
    profile: dataUrl(await readFile(path.join(directory, "avatar.png")), "image/png"),
    gallery: dataUrl(await readFile(path.join(directory, "qqshow.jpg")), "image/jpeg"),
  };
  const preview = {
    homeTitle: manifest.name,
    homeSubtitle: manifest.description,
    suggestionOne: "分析当前项目结构",
    suggestionTwo: "实现一个完整功能",
    projectName: "主题预览工作台",
    userMessage: "请检查这个主题的界面效果。",
    assistantMessage: "我会检查颜色、背景、控件和响应式表现。",
    composerPlaceholder: "向 Codex 描述你的任务",
    ...(manifest.previewContent && typeof manifest.previewContent === "object" ? manifest.previewContent : {}),
  };
  const palettes = manifest.colors && typeof manifest.colors === "object" ? manifest.colors : {};
  const initialColors = palettes[initialMode] && typeof palettes[initialMode] === "object" ? palettes[initialMode] : palettes;
  const tokens = { background: "--skin-bg", panel: "--skin-panel", panelAlt: "--skin-panel-alt", accent: "--skin-accent", accentAlt: "--skin-accent-alt", text: "--skin-text", muted: "--skin-muted", line: "--skin-line" };
  const initialStyle = Object.entries(tokens).filter(([key]) => typeof initialColors[key] === "string" && initialColors[key]).map(([key, token]) => `${token}:${escape(initialColors[key])};`).join("");
  const navItems = [["home", "navNewTask", "新任务"], ["pull-requests", "navPullRequests", "拉取请求"], ["sites", "navSites", "站点"], ["automations", "navAutomations", "已安排"], ["plugins", "navPlugins", "插件"], ["settings", "navSettings", "设置"]];
  const nav = navItems.map(([surface, field, fallback]) => `<button type="button" data-surface="${surface}"${surface === initialSurface ? ' aria-current="page"' : ""}>${escape(preview[field] || fallback)}</button>`).join("");
  const hidden = (surface) => surface !== initialSurface;
  const surfaces = SURFACES.map((surface) => surfaceMarkup(surface, preview, assets, hidden)).join("\n");
  const extensionRoot = pureTheme ? "" : `<div id="codex-dream-skin-chrome" data-surface="${initialSurface}" aria-hidden="true">${panelsMarkup(manifest, assets)}</div>`;
  const runtime = JSON.stringify({ colors: manifest.colors ?? {}, art: assets.art, modeBackgrounds }).replaceAll("</script", "<\\/script");
  return `<!doctype html>
<html class="codex-dream-skin" data-dream-shell="${initialMode}" data-dream-surface="${initialSurface}" style="${initialStyle}" lang="zh-CN">
<head><meta charset="utf-8"><meta name="viewport" content="width=device-width,initial-scale=1"><title>${escape(manifest.name)} - 设计预览</title>
<style>
*{box-sizing:border-box}html,body{margin:0;width:100%;min-width:320px;min-height:100%;font:14px/1.5 -apple-system,BlinkMacSystemFont,"Segoe UI",sans-serif}body{min-height:100vh;overflow:hidden}button,textarea{font:inherit}.preview-note{position:fixed;z-index:100;left:50%;top:8px;transform:translateX(-50%);padding:4px 9px;border-radius:4px;background:#111d;color:#fff;font-size:11px;pointer-events:none}.preview-controls{position:fixed;z-index:100;right:12px;bottom:12px;display:flex;flex-wrap:wrap;justify-content:flex-end;max-width:min(760px,calc(100vw - 24px));gap:6px;padding:6px;border:1px solid #7778;border-radius:6px;background:#111d}.preview-controls button{min-height:32px;padding:4px 10px;border:1px solid #aaa8;border-radius:4px;background:#292929;color:#fff}#root{display:grid;grid-template-columns:244px minmax(0,1fr);width:100vw;height:100vh}aside.app-shell-left-panel{position:relative;z-index:2;padding:16px 10px;overflow:hidden auto}.preview-skin-title{margin:0 8px 18px;font-size:15px}.preview-skin-description{margin:-12px 8px 16px;color:var(--skin-muted);font-size:12px}aside nav{display:grid;gap:5px}aside nav button{display:block;width:100%;min-height:36px;padding:7px 10px;border:0;text-align:left;color:inherit;background:transparent}main.main-surface{position:relative;min-width:0;overflow:hidden}header.app-header-tint{height:48px;display:flex;align-items:center;justify-content:space-between;padding:0 18px}.preview-surface{height:calc(100vh - 48px);overflow:auto;padding:34px clamp(18px,7vw,96px) 110px}.preview-home{display:grid;align-content:center;min-height:calc(100vh - 160px);text-align:center}.preview-home h1{margin:0;font-size:clamp(28px,5vw,48px)}.preview-home p{margin:8px auto 24px;max-width:620px;color:var(--skin-muted)}.group\\/home-suggestions{display:grid;grid-template-columns:repeat(2,minmax(0,1fr));gap:10px;max-width:720px;margin:auto}.group\\/home-suggestions button{min-height:72px;padding:12px;text-align:left}.thread-scroll-container{max-width:820px;margin:auto;display:grid;gap:16px}[data-message-author-role]{max-width:78%;padding:12px 14px;border:1px solid var(--skin-line);border-radius:var(--skin-radius);background:var(--skin-panel)}[data-message-author-role=user]{justify-self:end;background:var(--skin-panel-alt)}.composer-surface-chrome{position:absolute;z-index:3;left:50%;bottom:24px;width:min(760px,calc(100% - 40px));transform:translateX(-50%);padding:10px 12px}.ProseMirror{min-height:52px}.preview-feature{padding:0 0 76px}.preview-feature h1,.preview-feature h2,.preview-feature p{margin:0}.preview-feature-header{min-height:64px;display:flex;align-items:center;justify-content:space-between;gap:16px;padding:12px 18px;border-bottom:1px solid var(--skin-line)}.preview-feature-header input{width:min(280px,42vw);padding:8px 10px}.preview-split{display:grid;grid-template-columns:minmax(220px,.8fr) 1px minmax(300px,1.2fr);min-height:calc(100vh - 112px)}.preview-split>:where([role=list],form,.preview-detail){padding:16px}.preview-pr-layout{grid-template-columns:minmax(260px,.9fr) 1px minmax(360px,1.35fr)}.preview-empty{min-height:220px;display:grid;place-content:center;justify-items:center;gap:8px;text-align:center}.preview-card-grid{display:grid;grid-template-columns:repeat(auto-fill,minmax(220px,1fr));gap:14px;padding:18px}.preview-card-grid>[role=listitem]{padding:14px;border:1px solid var(--skin-line)}.preview-card-grid img,.preview-thumbnail{width:100%;aspect-ratio:16/9;margin-bottom:10px;object-fit:cover}.automation-row{display:grid;gap:2px;padding:10px}.preview-split form,.preview-settings{display:grid;align-content:start;gap:14px}.preview-split label,.preview-settings label{display:grid;gap:6px}.preview-settings-layout{display:grid;grid-template-columns:220px minmax(0,1fr);min-height:calc(100vh - 112px)}.preview-settings-layout nav{display:grid;align-content:start;gap:4px;padding:16px;border-right:1px solid var(--skin-line)}.preview-settings-layout nav button{padding:8px;border:0;text-align:left;background:transparent}.preview-settings{max-width:720px;padding:22px}.preview-plugin-icon{display:grid;place-items:center;width:40px;height:40px;margin-bottom:12px;border:1px solid var(--skin-line);border-radius:10px}[hidden]{display:none!important}@media(max-width:900px){#root{grid-template-columns:76px minmax(0,1fr)}.preview-skin-title,.preview-skin-description{display:none}.group\\/home-suggestions{grid-template-columns:1fr}.preview-split,.preview-pr-layout,.preview-settings-layout{grid-template-columns:1fr}.preview-split>[role=separator],.preview-split>:last-child,.preview-settings-layout>nav{display:none}.preview-card-grid{grid-template-columns:1fr}}
.preview-split [role="switch"],.preview-settings [role="switch"]{justify-self:start;width:auto;min-width:96px;padding-inline:12px}
.preview-app-menu{display:flex;align-items:center;gap:6px;height:42px;padding:5px 8px}.preview-app-menu button{min-width:42px;padding:3px 12px}body:has(>.preview-app-menu) #root{height:calc(100vh - 42px)}
#root{display:flex;flex-direction:column}.preview-workspace{display:grid;grid-template-columns:244px minmax(0,1fr);min-height:0;flex:1}.preview-workspace main.main-surface{min-height:0}.preview-surface{height:calc(100vh - 90px)}.preview-app-menu>div{display:flex;align-items:center;gap:4px}.preview-settings-surface{min-height:100%}.preview-settings section{display:grid;gap:8px}.preview-settings-card>div{display:flex;align-items:center;justify-content:space-between;min-height:52px;padding:10px 14px}.preview-settings-card>div:not(:last-child){position:relative}.preview-settings-card>div:not(:last-child)::after{content:"";position:absolute;inset-inline:14px;bottom:0;height:1px}.preview-settings [role="switch"]{width:32px;min-width:32px;height:20px;padding:0}.preview-settings [role="switch"]>span{position:relative;display:block;width:32px;height:20px;border-radius:999px}.preview-settings [role="switch"]>span>span{position:absolute;top:1px;width:16px;height:16px;border-radius:999px}.preview-settings [role="switch"][aria-checked="false"]>span>span{left:1px}.preview-settings [role="switch"][aria-checked="true"]>span>span{right:1px}@media(max-width:900px){.preview-workspace{grid-template-columns:76px minmax(0,1fr)}}
.preview-feature>section>h2{padding:12px 18px}.preview-plugin-search-chrome{position:sticky;top:0;z-index:30;padding:14px 20px;background:#181818}.preview-plugin-search-chrome::after{content:"";position:absolute;top:100%;left:0;width:100%;height:72px;background:linear-gradient(#181818,transparent);pointer-events:none}.preview-plugin-search-chrome>div{max-width:768px;margin:auto}.preview-plugin-search-chrome input{width:100%;padding:8px 10px}
${skinCss}
.preview-controls button{color:#fff!important}
</style></head><body><div class="preview-note">设计预览，不代表真实 Codex DOM 兼容性</div><div id="root"><div class="preview-app-menu"><div><button data-app-shell-sidebar-trigger aria-label="侧栏">▣</button><button aria-label="后退">←</button><button aria-label="前进" disabled>→</button></div><div role="menubar" aria-label="应用程序菜单"><button role="menuitem" id="application-menu-trigger-file-menu">File</button><button role="menuitem" id="application-menu-trigger-edit-menu">Edit</button><button role="menuitem" id="application-menu-trigger-view-menu">View</button><button role="menuitem" id="application-menu-trigger-help-menu">Help</button></div></div><div class="preview-workspace"><aside class="app-shell-left-panel"><h2 class="preview-skin-title">${escape(manifest.name)}</h2><p class="preview-skin-description">${escape(manifest.description)}</p><nav>${nav}</nav></aside><main class="main-surface"><header class="app-header-tint" data-app-shell-application-menu-bar><b>${escape(preview.projectName)}</b><span>${escape(manifest.author)}</span></header>${surfaces}<div class="composer-surface-chrome" data-preview-composer${["home", "chat"].includes(initialSurface) ? "" : " hidden"}><div class="ProseMirror" contenteditable="true">${escape(preview.composerPlaceholder)}</div></div></main></div></div>${extensionRoot}<div class="preview-controls" aria-label="预览控制"><button data-mode="dark">深色</button><button data-mode="light">浅色</button>${SURFACES.map((surface) => `<button data-surface="${surface}">${surface}</button>`).join("")}</div>
<script>const previewData=${runtime};const root=document.documentElement;const previewChrome=document.getElementById('codex-dream-skin-chrome');const surfaces=${JSON.stringify(SURFACES)};const tokens={background:'--skin-bg',panel:'--skin-panel',panelAlt:'--skin-panel-alt',accent:'--skin-accent',accentAlt:'--skin-accent-alt',text:'--skin-text',muted:'--skin-muted',line:'--skin-line'};function mode(value){root.dataset.dreamShell=value;const colors=previewData.colors?.[value]??previewData.colors??{};for(const[key,token]of Object.entries(tokens))if(colors[key])root.style.setProperty(token,colors[key]);const background=previewData.modeBackgrounds?.[value]||previewData.art;if(background)root.style.setProperty('--skin-background-image','url('+JSON.stringify(background)+')')}function surface(value){if(!surfaces.includes(value))return;root.dataset.dreamSurface=value;if(previewChrome)previewChrome.dataset.surface=value;document.querySelectorAll('.preview-surface').forEach((node)=>node.hidden=node.id!==value);document.querySelector('[data-preview-composer]').hidden=!['home','chat'].includes(value);document.querySelectorAll('aside [data-surface]').forEach((button)=>button.toggleAttribute('aria-current',button.dataset.surface===value))}function position(){if(!previewChrome)return;const main=document.querySelector('main.main-surface').getBoundingClientRect();Object.assign(previewChrome.style,{left:main.left+'px',top:main.top+'px',width:main.width+'px',height:main.height+'px'});previewChrome.querySelectorAll('.skin-photo-panel').forEach((panel)=>panel.hidden=innerWidth<Number(panel.dataset.minViewportWidth||1280))}root.style.setProperty('--dream-skin-art','url('+JSON.stringify(previewData.art)+')');document.querySelectorAll('[data-mode]').forEach((button)=>button.addEventListener('click',()=>mode(button.dataset.mode)));document.querySelectorAll('[data-surface]').forEach((button)=>button.addEventListener('click',()=>surface(button.dataset.surface)));addEventListener('resize',position,{passive:true});mode(${JSON.stringify(initialMode)});surface(${JSON.stringify(initialSurface)});position();</script></body></html>`;
}

async function main() {
  const { values, positional } = parseArgs(process.argv.slice(2), { valueOptions: ["output", "surface", "mode"], flags: ["force"] });
  if (positional.length !== 1) throw new Error("用法：node scripts/render_preview.mjs <皮肤目录> --output <HTML 路径>");
  const directory = path.resolve(positional[0]);
  const output = path.resolve(requireValue(values, "output", "预览 HTML 输出路径"));
  const surface = values.surface ?? "chat";
  const mode = values.mode ?? "dark";
  if (!SURFACES.includes(surface)) throw new Error(`--surface 必须是：${SURFACES.join(", ")}`);
  if (!["dark", "light"].includes(mode)) throw new Error("--mode 必须是 dark 或 light。");
  if (await exists(output) && !values.force) throw new Error(`预览已存在；确认覆盖时增加 --force：${output}`);
  await ensureDirectory(path.dirname(output));
  await writeFile(output, await buildPreview(directory, surface, mode), "utf8");
  console.log(`已生成设计预览：${output}`);
}

main().catch((error) => {
  console.error(`预览生成失败：${error.message}`);
  process.exitCode = 1;
});
