import { describe, expect, test, vi } from "vitest";

const invokeMock = vi.hoisted(() => vi.fn());
vi.mock("@tauri-apps/api/core", () => ({ invoke: invokeMock }));

import {
  decodeAppMetadata,
  getAppMetadata,
  getSystemLocale,
  setInterfaceLanguage,
} from "../src/lib/api";

describe("Tauri API boundary", () => {
  /** 验证应用元数据命令和严格返回结构。 */
  test("loads validated application metadata", async () => {
    invokeMock.mockResolvedValue({
      applicationName: "LokiMetis",
      productDefinitionRequired: true,
      title: "LokiMetis v0.1.0",
      version: "0.1.0",
    });

    await expect(getAppMetadata()).resolves.toMatchObject({
      applicationName: "LokiMetis",
      productDefinitionRequired: true,
    });
    expect(invokeMock).toHaveBeenCalledWith("get_app_metadata");
  });

  /** 验证不完整的宿主元数据不会进入界面。 */
  test("rejects malformed metadata", () => {
    expect(() => decodeAppMetadata({ applicationName: "LokiMetis" })).toThrow();
  });

  /** 验证语言切换使用固定参数名并收窄宿主响应。 */
  test("sends the interface language through the narrow command", async () => {
    invokeMock.mockResolvedValue("zh-CN");
    await expect(setInterfaceLanguage("zh-CN")).resolves.toBe("zh-CN");
    expect(invokeMock).toHaveBeenCalledWith("set_interface_language", {
      language: "zh-CN",
    });
  });

  /** 验证系统语言也经过 unknown 边界收窄。 */
  test("decodes the system locale from its fixed command", async () => {
    invokeMock.mockResolvedValue("en-US");
    await expect(getSystemLocale()).resolves.toBe("en-US");
    expect(invokeMock).toHaveBeenCalledWith("get_system_locale");
  });
});
