import { act, renderHook, waitFor } from "@testing-library/react";
import { invoke } from "@tauri-apps/api/core";
import type { ReactNode } from "react";
import { beforeEach, describe, expect, test, vi } from "vitest";

import type { SkinDescriptor } from "../src/api/skins";
import { useSkinHostActions } from "../src/pages/useSkinHostActions";
import { TestProviders } from "./testUtils";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));

const invokeMock = vi.mocked(invoke);
const guardedSkin: SkinDescriptor = {
  author: "ManonLoki",
  id: "guarded-theme",
  name: "Guarded theme",
  packageType: "theme",
  previewDataUrl: "",
  source: "user",
  supportedColorModes: ["light", "dark"],
  version: "1.0.0",
};

/** 为宿主动作 hook 提供可切换的当前权威代次。 */
function renderActions(initialReady = true) {
  const refreshHost = vi.fn().mockResolvedValue(undefined);
  const setNotice = vi.fn();
  const wrapper = ({ children }: { children: ReactNode }) => (
    <TestProviders>{children}</TestProviders>
  );
  return {
    ...renderHook(
      ({ ready }: { ready: boolean }) =>
        useSkinHostActions({
          activeHost: "codex",
          host: "codex",
          hostStateReady: ready,
          instanceList: [],
          refreshHost,
          setNotice,
        }),
      { initialProps: { ready: initialReady }, wrapper },
    ),
    refreshHost,
  };
}

describe("skin host action authority", () => {
  beforeEach(() => {
    invokeMock.mockReset();
  });

  /** 启动宿主期间失权再恢复，旧安装编排也不得继续读取实例或安装。 */
  test("does not continue an install after its authority epoch changed", async () => {
    let resolveLaunch: ((value: { state: "ready" }) => void) | undefined;
    const launch = new Promise<{ state: "ready" }>((resolve) => {
      resolveLaunch = resolve;
    });
    invokeMock.mockImplementation((command: string) => {
      if (command === "launch_skin_host") return launch;
      throw new Error(`unexpected command: ${command}`);
    });
    const { result, rerender } = renderActions();
    let operation: Promise<void> | undefined;

    act(() => {
      operation = result.current.requestInstall(guardedSkin, false);
    });
    await waitFor(() =>
      expect(invokeMock).toHaveBeenCalledWith("launch_skin_host", { host: "codex" }),
    );
    rerender({ ready: false });
    rerender({ ready: true });
    resolveLaunch?.({ state: "ready" });

    await expect(operation).rejects.toMatchObject({ code: "skin.host_state_unavailable" });
    expect(invokeMock).not.toHaveBeenCalledWith(
      "list_skin_host_instances",
      expect.anything(),
    );
    expect(invokeMock).not.toHaveBeenCalledWith("install_skin", expect.anything());
  });

  /** 实例重启等待期间失权再恢复，旧确认不能以新代次继续安装。 */
  test("does not install after a restarted instance crossed authority epochs", async () => {
    let resolveRestart:
      | ((value: {
          accountLabel: null;
          activeSkin: null;
          activeSkinName: null;
          avatarDataUrl: null;
          debugPort: number;
          id: string;
          label: string;
          pid: number;
          profile: null;
          state: "ready";
        }) => void)
      | undefined;
    const restart = new Promise<{
      accountLabel: null;
      activeSkin: null;
      activeSkinName: null;
      avatarDataUrl: null;
      debugPort: number;
      id: string;
      label: string;
      pid: number;
      profile: null;
      state: "ready";
    }>((resolve) => {
      resolveRestart = resolve;
    });
    invokeMock.mockImplementation((command: string) => {
      if (command === "restart_skin_host_instance") return restart;
      throw new Error(`unexpected command: ${command}`);
    });
    const { result, rerender } = renderActions();
    let operation: Promise<void> | undefined;

    act(() => {
      operation = result.current.restartAndInstall(
        "codex",
        guardedSkin,
        "codex-old",
        false,
      );
    });
    await waitFor(() =>
      expect(invokeMock).toHaveBeenCalledWith("restart_skin_host_instance", {
        host: "codex",
        instanceId: "codex-old",
      }),
    );
    rerender({ ready: false });
    rerender({ ready: true });
    resolveRestart?.({
      accountLabel: null,
      activeSkin: null,
      activeSkinName: null,
      avatarDataUrl: null,
      debugPort: 9222,
      id: "codex-new",
      label: "Codex",
      pid: 42,
      profile: null,
      state: "ready",
    });

    await expect(operation).rejects.toMatchObject({ code: "skin.host_state_unavailable" });
    expect(invokeMock).not.toHaveBeenCalledWith("install_skin", expect.anything());
  });
});
