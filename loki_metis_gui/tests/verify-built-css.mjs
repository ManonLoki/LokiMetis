import { readdir, readFile } from "node:fs/promises";
import { join, relative } from "node:path";
import { fileURLToPath } from "node:url";

const projectRoot = fileURLToPath(new URL("../", import.meta.url));
const sourceRoot = join(projectRoot, "src");
const builtAssetsRoot = join(projectRoot, "dist", "assets");

/** 递归收集目录里的 CSS 文件，避免遗漏按路由拆分的样式。 */
async function collectCssFiles(directory) {
  const entries = await readdir(directory, { withFileTypes: true });
  const nestedFiles = await Promise.all(
    entries.map(async (entry) => {
      const path = join(directory, entry.name);
      if (entry.isDirectory()) return collectCssFiles(path);
      return entry.isFile() && entry.name.endsWith(".css") ? [path] : [];
    }),
  );
  return nestedFiles.flat();
}

/** 返回包含指定生产样式风险的相对文件路径。 */
async function findOffenders(files, pattern) {
  const offenders = [];
  for (const file of files) {
    const css = await readFile(file, "utf8");
    if (pattern.test(css)) offenders.push(relative(projectRoot, file));
    pattern.lastIndex = 0;
  }
  return offenders;
}

const sourceFiles = await collectCssFiles(sourceRoot);
if (sourceFiles.length === 0) throw new Error("未找到待检查的前端 CSS 源文件");

const lightDarkSources = await findOffenders(sourceFiles, /light-dark\s*\(/gu);
if (lightDarkSources.length > 0) {
  throw new Error(
    `第一方 CSS 必须使用应用语义变量，不能使用 light-dark(): ${lightDarkSources.join(", ")}`,
  );
}

const builtFiles = await collectCssFiles(builtAssetsRoot);
if (builtFiles.length === 0) throw new Error("生产构建没有生成 CSS 资源");

const unsafeBuiltFiles = await findOffenders(
  builtFiles,
  /--lightningcss-(?:light|dark)\b/gu,
);
if (unsafeBuiltFiles.length > 0) {
  throw new Error(
    `生产 CSS 含有不安全的 Lightning CSS 亮暗辅助变量: ${unsafeBuiltFiles.join(", ")}`,
  );
}

console.log(
  `生产 CSS 检查通过（源码 ${sourceFiles.length} 个，产物 ${builtFiles.length} 个）`,
);
