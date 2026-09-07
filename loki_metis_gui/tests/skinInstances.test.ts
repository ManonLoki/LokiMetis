import { describe, expect, test } from "vitest";

import type { CodexInstance } from "../src/api/skins";
import { resolveSoleTargetInstance } from "../src/lib/skin-instances";

/** 构造无需访问真实宿主的换皮实例快照。 */
function hostInstance(id: string): CodexInstance {
  return {
    accountLabel: null,
    activeSkin: null,
    activeSkinName: null,
    avatarDataUrl: null,
    debugPort: 9222,
    id,
    label: `Codex ${id}`,
    pid: Number(id),
    profile: null,
    state: "ready",
  };
}

describe("skin target resolution", () => {
  /** 无选择控件时只接受唯一实例，多个实例不得按列表顺序静默命中。 */
  test("resolves_only_a_sole_host_instance", () => {
    const first = hostInstance("1");
    const second = hostInstance("2");

    expect(resolveSoleTargetInstance([])).toBeNull();
    expect(resolveSoleTargetInstance([first])).toBe(first);
    expect(resolveSoleTargetInstance([first, second])).toBeNull();
  });
});
