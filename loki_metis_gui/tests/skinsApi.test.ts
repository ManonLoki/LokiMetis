import { beforeEach, describe, expect, test, vi } from "vitest";

const mocks = vi.hoisted(() => ({
  channels: [] as Array<{ onmessage: (event: unknown) => void }>,
  invoke: vi.fn(),
}));

vi.mock("@tauri-apps/api/core", () => ({
  Channel: class MockChannel {
    onmessage: (event: unknown) => void;

    /** 保存测试回调，使增量预检事件可以被精确断言。 */
    constructor(onmessage: (event: unknown) => void) {
      this.onmessage = onmessage;
      mocks.channels.push(this);
    }
  },
  invoke: mocks.invoke,
  isTauri: () => true,
}));

vi.mock("@tauri-apps/api/webview", () => ({
  getCurrentWebview: () => ({ onDragDropEvent: vi.fn() }),
}));

import { skinApi } from "../src/api/skins";

describe("typed skin host API", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    mocks.channels.length = 0;
  });

  /** 锁定资源扫描和创建主题的最小 IPC 参数，避免 WebView 注入本机路径。 */
  test("catalog and creator use typed commands", async () => {
    mocks.invoke.mockResolvedValueOnce([]).mockResolvedValueOnce({ id: "theme_1" });
    await skinApi.list();
    await skinApi.createTheme("Night", "Loki");
    expect(mocks.invoke).toHaveBeenNthCalledWith(1, "list_skins", undefined);
    expect(mocks.invoke).toHaveBeenNthCalledWith(2, "create_user_theme", {
      author: "Loki",
      name: "Night",
    });
  });

  /** 锁定多实例应用必须携带精确来源、ID 与显式目标。 */
  test("install targets the selected instance", async () => {
    mocks.invoke.mockResolvedValue({ type: "installed", status: {} });
    await skinApi.install(
      "codex",
      { id: "minecraft", source: "builtin" },
      false,
      "codex-42",
    );
    expect(mocks.invoke).toHaveBeenCalledWith("install_skin", {
      allowAppearanceMismatch: false,
      host: "codex",
      instanceId: "codex-42",
      skin: { id: "minecraft", source: "builtin" },
    });
  });

  /** 锁定 WorkBuddy 风险确认后的恢复与安装由同一命令完成，不接受前端 PID 或路径。 */
  test("confirmed workbuddy recovery is atomic with installation", async () => {
    mocks.invoke.mockResolvedValueOnce(true).mockResolvedValueOnce({
      type: "installed",
      status: {},
    });
    await expect(skinApi.supportsWindowsWorkBuddyRecovery()).resolves.toBe(true);
    await skinApi.install(
      "workBuddy",
      { id: "minecraft", source: "builtin" },
      false,
      null,
      true,
    );
    expect(mocks.invoke).toHaveBeenNthCalledWith(
      1,
      "supports_windows_workbuddy_recovery",
      undefined,
    );
    expect(mocks.invoke).toHaveBeenNthCalledWith(2, "install_skin", {
      allowAppearanceMismatch: false,
      allowWorkBuddyRecovery: true,
      host: "workBuddy",
      skin: { id: "minecraft", source: "builtin" },
    });
  });

  /** 验证导入预检进度只通过类型化 Channel 回传。 */
  test("import preflight streams bounded progress", async () => {
    const onProgress = vi.fn();
    mocks.invoke.mockResolvedValue(null);
    await skinApi.prepareImport(onProgress);
    expect(mocks.invoke).toHaveBeenCalledWith("prepare_skin_import", {
      onProgress: mocks.channels[0],
    });
    const event = { token: "batch-1", totalFiles: 2, type: "started" };
    mocks.channels[0]?.onmessage(event);
    expect(onProgress).toHaveBeenCalledWith(event);
  });

  /** 验证批量删除只传递核心层可校验的精确引用集合。 */
  test("batch delete carries exact references", async () => {
    const skins = [
      { id: "one", source: "user" as const },
      { id: "two", source: "user" as const },
    ];
    mocks.invoke.mockResolvedValue({ deleted: skins, failed: [] });
    await skinApi.deleteMany(skins);
    expect(mocks.invoke).toHaveBeenCalledWith("delete_skins", { skins });
  });
});
