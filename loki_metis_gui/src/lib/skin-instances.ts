import type { CodexInstance } from "../api/skins";

/** 只在宿主恰有一个实例时返回目标，避免无选择控件时静默命中其他进程。 */
export function resolveSoleTargetInstance(
  instances: CodexInstance[],
): CodexInstance | null {
  return instances.length === 1 ? (instances.at(0) ?? null) : null;
}
