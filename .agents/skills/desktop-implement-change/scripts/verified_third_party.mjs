/** 仅凭封闭登记清单及逐文件原样字节，确认不属于人工维护范围的第三方文件。 */

import { createHash } from "node:crypto";
import { closeSync, constants, fstatSync, lstatSync, openSync, readFileSync } from "node:fs";
import path from "node:path";

const REGISTERED_SOURCES = Object.freeze([
  Object.freeze({
    manifest: "vendor/tray-icon/source-content.sha256",
    root: "vendor/tray-icon/upstream",
    sha256: "43d8cea392f1ebe242cb4845548343384739e07fe43f35de7c09b07ed4bbbdc9",
    entries: 26,
  }),
  Object.freeze({
    manifest: "vendor/accesskit_macos/source-content.sha256",
    sha256: "f4bc40427a5db1b6acf7262cf8d4bcdfe25c19b6a16499302252a3942e86ffda",
    entries: 18,
  }),
  Object.freeze({
    manifest: "vendor/gpui-pre/source-content.sha256",
    root: "vendor/gpui-pre/upstream",
    sha256: "34525d22e585a0e44848e9440efc127874fe14703cca9b7cae49bab5cc7bb163",
    entries: 163,
  }),
  Object.freeze({
    manifest: "vendor/gpui-pre-apple/source-content.sha256",
    root: "vendor/gpui-pre-apple/upstream",
    sha256: "6192d5cc95ea36501eee39d04a373459711161ee75f3af65ffca1102e264793a",
    entries: 19,
  }),
  Object.freeze({
    manifest: "vendor/gpui-pre-macos/source-content.sha256",
    root: "vendor/gpui-pre-macos/upstream",
    sha256: "185befb334e14f8d874b23a134e8bb6e35894a48cd86e205f3754a3d296f7c9f",
    entries: 15,
  }),
  Object.freeze({
    manifest: "vendor/gpui-pre-windows/source-content.sha256",
    root: "vendor/gpui-pre-windows/upstream",
    sha256: "67ceabff1707a47d93fb10d594fa27a8d6928f945cb839a9d958f971bdf7a9df",
    entries: 28,
  }),
  Object.freeze({
    manifest: "vendor/gpui-pre-wgpu/source-content.sha256",
    root: "vendor/gpui-pre-wgpu/upstream",
    sha256: "55ff8930ca0210d861b0a0002a2bb997be332bc1acfd0cda62f778ef8a7ca23b",
    entries: 14,
  }),
]);

function digest(payload) { return createHash("sha256").update(payload).digest("hex"); }

function relativeSegments(relative) {
  const segments = relative.split("/");
  if (!relative || path.posix.isAbsolute(relative) || /^[a-z]:/iu.test(relative)
      || /[\\\0\r\n]/u.test(relative) || segments.some((segment) => !segment || segment === "." || segment === "..")) {
    throw new Error(`清单路径越界或不规范: ${JSON.stringify(relative)}`);
  }
  return segments;
}

/** 解析器不授予排除权限；调用方仍须通过固定清单摘要及每项文件校验。 */
export function parseThirdPartyManifest(payload) {
  const lines = new TextDecoder("utf-8", { fatal: true }).decode(payload).split("\n");
  if (lines.at(-1) === "") lines.pop();
  const seen = new Set();
  return lines.map((line) => {
    const match = /^([a-f0-9]{64})  (.+)$/u.exec(line);
    if (!match) throw new Error("清单条目不是 SHA256 双空格相对路径格式");
    const [, sha256, relative] = match;
    relativeSegments(relative);
    if (seen.has(relative)) throw new Error(`清单包含重复路径: ${relative}`);
    seen.add(relative);
    return { relative, sha256 };
  });
}

/** 逐级检查普通目录，拒绝文件自身或任一父目录的链接。 */
function inspectPath(root, relative, directory = false, allowMissing = false) {
  const segments = relativeSegments(relative);
  let candidate = root;
  let stat;
  for (let index = 0; index < segments.length; index += 1) {
    candidate = path.join(candidate, segments[index]);
    try { stat = lstatSync(candidate); } catch (error) {
      if (error?.code === "ENOENT" && allowMissing) return null;
      if (error?.code === "ENOENT" || error?.code === "ENOTDIR") throw new Error(`清单文件或父目录缺失: ${relative}`);
      throw error;
    }
    if (stat.isSymbolicLink()) throw new Error(`清单文件或父目录不得是符号链接: ${relative}`);
    const needsDirectory = directory || index < segments.length - 1;
    if (needsDirectory ? !stat.isDirectory() : !stat.isFile()) {
      throw new Error(`清单路径不是普通${needsDirectory ? "目录" : "文件"}: ${relative}`);
    }
  }
  return { candidate, stat };
}

function readPlainFile(root, relative) {
  const { candidate, stat } = inspectPath(root, relative);
  const descriptor = openSync(candidate, constants.O_RDONLY | (constants.O_NOFOLLOW ?? 0));
  try {
    const opened = fstatSync(descriptor);
    if (!opened.isFile() || opened.dev !== stat.dev || opened.ino !== stat.ino) throw new Error(`校验期间文件发生变化: ${relative}`);
    const payload = readFileSync(descriptor);
    const checked = inspectPath(root, relative).stat;
    if (checked.dev !== opened.dev || checked.ino !== opened.ino) throw new Error(`校验期间文件发生变化: ${relative}`);
    return payload;
  } finally { closeSync(descriptor); }
}

/** 根须为调用方已确认的真实目录；未接入登记组件的仓库不产生第三方排除项。 */
export function verifyRegisteredThirdParty(root) {
  const files = [];
  const errors = [];
  for (const registration of REGISTERED_SOURCES) {
    const directory = path.posix.dirname(registration.manifest);
    try {
      if (!inspectPath(root, directory, true, true)) continue;
      const payload = readPlainFile(root, registration.manifest);
      if (digest(payload) !== registration.sha256) throw new Error("清单 SHA256 与封闭登记不符");
      const entries = parseThirdPartyManifest(payload);
      if (entries.length !== registration.entries) throw new Error(`清单必须恰有 ${registration.entries} 项`);
      const verified = [];
      for (const entry of entries) {
        const relative = `${registration.root ?? directory}/${entry.relative}`;
        try {
          if (digest(readPlainFile(root, relative)) !== entry.sha256) throw new Error(`原样文件 SHA256 不符: ${relative}`);
          verified.push(relative);
        } catch (error) { errors.push(`原样第三方校验失败 (${registration.manifest}): ${error.message}`); }
      }
      // 一个登记来源必须完整通过；不允许部分文件在同一失败清单下被豁免。
      if (verified.length === entries.length) files.push(...verified);
    } catch (error) { errors.push(`原样第三方校验失败 (${registration.manifest}): ${error.message}`); }
  }
  return { files: files.sort(), errors: errors.sort() };
}
