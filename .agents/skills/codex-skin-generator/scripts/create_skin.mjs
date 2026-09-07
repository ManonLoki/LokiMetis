#!/usr/bin/env node
import { readdir, readFile, stat, writeFile } from "node:fs/promises";
import path from "node:path";
import { copy, ensureDirectory, exists, parseArgs, requireValue, writeJson } from "./lib.mjs";

const ID_PATTERN = /^[a-z0-9_-]{1,64}$/;
const LIGHT_PALETTE = `  --skin-bg: #E7F4F5; --skin-panel: #F8FCFC; --skin-panel-alt: #D9ECEE;
  --skin-accent: #087B65; --skin-accent-alt: #176DB0; --skin-text: #102A34;
  --skin-muted: #4C6871; --skin-line: #91ADB4;`;
const DARK_PALETTE = `  --skin-bg: #07131F; --skin-panel: #102738; --skin-panel-alt: #18394A;
  --skin-accent: #78F0C6; --skin-accent-alt: #78B9FF; --skin-text: #F4FAFF;
  --skin-muted: #B6CBD8; --skin-line: #416477;`;

function normalized(value, label, maximum) {
  const result = value.trim();
  if (!result || result.length > maximum) throw new Error(`${label}去除首尾空白后必须为 1 到 ${maximum} 个字符。`);
  return result;
}

async function imageKind(target) {
  const info = await stat(target).catch(() => null);
  if (!info?.isFile()) throw new Error(`图片不存在或不是文件：${target}`);
  if (info.size === 0 || info.size > 16 * 1024 * 1024) throw new Error(`图片必须非空且不超过 16 MiB：${target}`);
  const header = (await readFile(target)).subarray(0, 12);
  if (header.subarray(0, 8).equals(Buffer.from([0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a]))) return "png";
  if (header.subarray(0, 3).equals(Buffer.from([0xff, 0xd8, 0xff]))) return "jpg";
  throw new Error(`只支持真实 PNG 或 JPEG 图片：${target}`);
}

async function main() {
  const { values, positional } = parseArgs(process.argv.slice(2), {
    valueOptions: ["output", "id", "name", "description", "author", "hero-image", "color-modes"],
  });
  if (positional.length) throw new Error(`不支持位置参数：${positional.join(" ")}`);
  const output = path.resolve(requireValue(values, "output", "输出目录"));
  const themeId = requireValue(values, "id", "主题 ID");
  if (!ID_PATTERN.test(themeId)) throw new Error("主题 ID 只能包含 1 到 64 个小写字母、数字、连字符或下划线。");
  const name = normalized(requireValue(values, "name", "主题名称"), "主题名称", 80);
  const description = normalized(requireValue(values, "description", "主题说明"), "主题说明", 500);
  const author = normalized(requireValue(values, "author", "作者名称"), "作者名称", 80);
  const colorModes = values["color-modes"] ?? "both";
  if (!["both", "light", "dark"].includes(colorModes)) {
    throw new Error("颜色模式只接受 both、light 或 dark。");
  }
  const supportedColorModes = colorModes === "both" ? ["light", "dark"] : [colorModes];
  const background = path.resolve(requireValue(values, "hero-image", "背景图片"));
  const backgroundKind = await imageKind(background);
  if (await exists(output)) {
    const info = await stat(output);
    if (!info.isDirectory() || (await readdir(output)).length > 0) throw new Error(`输出目录必须不存在或为空，避免覆盖已有文件：${output}`);
  }
  await ensureDirectory(output);
  const backgroundFile = `background.${backgroundKind}`;
  const manifest = {
    schemaVersion: 3,
    type: "theme",
    id: themeId,
    name,
    description,
    author,
    appearance: {
      supportedColorModes,
    },
  };
  await writeJson(path.join(output, "theme.json"), manifest);
  await writeFile(path.join(output, "theme.css"), `html.codex-dream-skin {
  --skin-preview-image: url("${backgroundFile}");
  --skin-background-image: url("${backgroundFile}");
  --skin-background-size: cover;
  --skin-background-position-x: 50%;
  --skin-background-position-y: 50%;
  --skin-radius: 14px;
  --skin-blur: 20px;
  --skin-sidebar-opacity: 90%;
  --skin-main-opacity: 76%;
  --skin-header-opacity: 86%;
  --skin-composer-opacity: 94%;
  --skin-card-opacity: 88%;
  --skin-border-width: 1px;
  --skin-shadow-opacity: 72%;
  --skin-texture-opacity: 10%;
${colorModes === "dark" ? DARK_PALETTE : LIGHT_PALETTE}
}
`, "utf8");
  if (supportedColorModes.includes("light")) await writeFile(path.join(output, "theme.light.css"), `html.codex-dream-skin[data-dream-shell="light"] {
${LIGHT_PALETTE}
}
`, "utf8");
  if (supportedColorModes.includes("dark")) await writeFile(path.join(output, "theme.dark.css"), `html.codex-dream-skin[data-dream-shell="dark"] {
${DARK_PALETTE}
}
`, "utf8");
  await copy(background, path.join(output, backgroundFile));
  console.log(`已创建纯主题目录：${output}`);
  console.log(`下一步：按视觉意图调整 theme.css 基础色板与 ${supportedColorModes.join("、")} 模式 CSS，生成预览，再运行 validate_skin.mjs。`);
}

main().catch((error) => {
  console.error(`创建失败：${error.message}`);
  process.exitCode = 1;
});
