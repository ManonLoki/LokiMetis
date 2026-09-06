import type { SourceRootDto } from "./api/usage";

/** 同名数据根追加稳定内部 ID 后缀，避免展示绝对路径仍可区分多个 Codex home。 */
// 只在检测到列表里存在别名重复的其他根时才追加后缀（`root.id.slice(-8)`
// 取内部稳定 ID 的最后 8 位而不是完整 ID，够短又足以区分同名项）；
// 唯一的别名保持原样展示，不会无缘无故给用户看一串不必要的技术后缀。
export function displayRootAlias(root: SourceRootDto, roots: SourceRootDto[]): string {
  const duplicate = roots.some(
    (candidate) => candidate.id !== root.id && candidate.alias === root.alias,
  );
  return duplicate ? `${root.alias} · ${root.id.slice(-8)}` : root.alias;
}
