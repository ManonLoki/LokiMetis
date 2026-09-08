import { describe, expect, test } from "vitest";

import type { CodexInstance } from "../src/api/skins";
import {
  needsWorkBuddyCdpRecovery,
  resolveSoleTargetInstance,
} from "../src/lib/skin-instances";

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

  /** WorkBuddy 多根或唯一根无 CDP 时进入 close-all 确认，唯一就绪根可直接复用。 */
  test("recovers_when_workbuddy_has_no_unique_safe_cdp_target", () => {
    const first = {
      ...hostInstance("1"),
      debugPort: null,
      state: "runningWithoutCdp" as const,
    };
    const second = {
      ...hostInstance("2"),
      debugPort: null,
      state: "runningWithoutCdp" as const,
    };

    expect(needsWorkBuddyCdpRecovery([])).toBe(false);
    expect(needsWorkBuddyCdpRecovery([first])).toBe(true);
    expect(needsWorkBuddyCdpRecovery([first, second])).toBe(true);
    expect(needsWorkBuddyCdpRecovery([first, hostInstance("3")])).toBe(true);
    expect(needsWorkBuddyCdpRecovery([hostInstance("3"), hostInstance("4")])).toBe(true);
    expect(needsWorkBuddyCdpRecovery([hostInstance("3")])).toBe(false);
  });
});
