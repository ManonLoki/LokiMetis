import type { CodexInstance } from "../api/skins";

/** 解析目标 Codex 实例：优先命中已选中项，否则在唯一实例时回退到它。 */
export function resolveTargetInstance(
  instances: CodexInstance[],
  selectedId: string | null,
): CodexInstance | null {
  return (
    instances.find((item) => item.id === selectedId) ??
    (instances.length === 1 ? (instances.at(0) ?? null) : null)
  );
}
