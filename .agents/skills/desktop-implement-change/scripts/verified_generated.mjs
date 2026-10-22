/** 仅排除已封存、精确登记且与生成器完成回执相符的生成物。 */

import { createHash } from "node:crypto";
import { closeSync, constants, fstatSync, lstatSync, openSync, readFileSync } from "node:fs";
import path from "node:path";

const REGISTERED_ARTIFACTS = Object.freeze([
  Object.freeze({
    artifact: ".archify/architecture-lokimetris-20261011-2240/lokimetris.html",
    sha256: "bb76374b3cbbec041c220999955ba8345e1b88705c10015dc8969a8612da5f36",
    bytes: 773026,
    receipt: ".archify/architecture-lokimetris-20261011-2240/lokimetris.finalize.json",
    receiptSha256: "6b08bbf3d8c0adf1e0d72a8d919dd4d7081cb6dca781e7393ae25e2e8a0bde8d",
    receiptBytes: 35802,
    generator: '<meta name="generator" content="archify 3.0.1">',
  }),
  Object.freeze({
    artifact: ".archify/architecture-desktop-20261022-144530/desktop.html",
    sha256: "4663994d0a059b0a60d564a1199f9db4b774e51dea439651d8f6569cfcc41c84",
    bytes: 783111,
    receipt: ".archify/architecture-desktop-20261022-144530/review-2/desktop.finalize.json",
    receiptSha256: "5c66e85ac6925bcc8a8c0d37592de9599993583bfd1292c9b72edce2f1fb7d7f",
    receiptBytes: 38677,
    generator: '<meta name="generator" content="archify 3.0.1">',
  }),
]);

/** 摘要只接受实际文件字节，不使用回执自行声明的值建立信任。 */
function digest(payload) { return createHash("sha256").update(payload).digest("hex"); }

/** 检查完整路径链，拒绝任意父目录或文件链接及非普通文件。 */
function inspectPath(root, relative) {
  let candidate = root;
  const segments = relative.split("/");
  if (segments.some((segment) => !segment || segment === "." || segment === "..") || /[\\\0\r\n:]/u.test(relative)) {
    throw new Error(`生成物登记路径不规范: ${relative}`);
  }
  let stat;
  for (let index = 0; index < segments.length; index += 1) {
    candidate = path.join(candidate, segments[index]);
    try { stat = lstatSync(candidate); } catch (error) {
      if (error?.code === "ENOENT") return null;
      throw error;
    }
    if (stat.isSymbolicLink()) throw new Error(`生成物或回执路径不得是符号链接: ${relative}`);
    const directory = index < segments.length - 1;
    if (directory ? !stat.isDirectory() : !stat.isFile()) throw new Error(`生成物或回执路径不是普通${directory ? "目录" : "文件"}: ${relative}`);
  }
  return { candidate, stat };
}

/** 读前读后复核文件身份和大小，不跟随被替换的末端链接。 */
function readPlainFile(root, relative, bytes) {
  const entry = inspectPath(root, relative);
  if (!entry) throw new Error(`生成物或回执缺失: ${relative}`);
  if (entry.stat.size !== bytes) throw new Error(`生成物或回执字节数不符: ${relative}`);
  const descriptor = openSync(entry.candidate, constants.O_RDONLY | (constants.O_NOFOLLOW ?? 0));
  try {
    const opened = fstatSync(descriptor);
    if (!opened.isFile() || opened.dev !== entry.stat.dev || opened.ino !== entry.stat.ino || opened.size !== bytes) {
      throw new Error(`校验期间生成物或回执发生变化: ${relative}`);
    }
    const payload = readFileSync(descriptor);
    const checked = inspectPath(root, relative)?.stat;
    if (!checked || checked.dev !== opened.dev || checked.ino !== opened.ino || checked.size !== bytes || payload.length !== bytes) {
      throw new Error(`校验期间生成物或回执发生变化: ${relative}`);
    }
    return payload;
  } finally { closeSync(descriptor); }
}

/** 只有固定回执身份、成功终态及所有生成阶段均绑定同一字节才认可封存物。 */
function verifyReceipt(receipt, registration) {
  if (receipt.schemaVersion !== 1 || receipt.ok !== true || receipt.command !== "finalize" || receipt.status !== "pass") {
    throw new Error("生成器完成回执不是成功终态");
  }
  const records = [receipt.artifact, ...["deliver", "check", "browser-check"].map((stage) => receipt.stages?.[stage]?.receipt?.artifact)];
  for (const record of records) {
    if (record?.sha256 !== registration.sha256 || record?.bytes !== registration.bytes) throw new Error("生成器各阶段未绑定同一生成物");
    if (record.path !== undefined && !record.path.endsWith(`/${registration.artifact}`)) throw new Error("生成器回执未绑定登记路径");
  }
}

/** 未含登记文件的仓库不产生排除；部分缺失或任何漂移均失败关闭。 */
export function verifyRegisteredGenerated(root) {
  const files = [];
  const errors = [];
  for (const registration of REGISTERED_ARTIFACTS) {
    try {
      const artifact = inspectPath(root, registration.artifact);
      const receiptFile = inspectPath(root, registration.receipt);
      if (!artifact && !receiptFile) continue;
      const receiptPayload = readPlainFile(root, registration.receipt, registration.receiptBytes);
      if (digest(receiptPayload) !== registration.receiptSha256) throw new Error("生成器完成回执 SHA256 与封闭登记不符");
      const receipt = JSON.parse(new TextDecoder("utf-8", { fatal: true }).decode(receiptPayload));
      verifyReceipt(receipt, registration);
      const payload = readPlainFile(root, registration.artifact, registration.bytes);
      if (digest(payload) !== registration.sha256) throw new Error("生成物 SHA256 与封闭登记及完成回执不符");
      if (!payload.includes(Buffer.from(registration.generator))) throw new Error("生成物缺少已登记生成器标记");
      files.push(registration.artifact);
    } catch (error) { errors.push(`封存生成物校验失败 (${registration.artifact}): ${error.message}`); }
  }
  return { files: files.sort(), errors: errors.sort() };
}
