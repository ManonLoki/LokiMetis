import { describe, expect, test } from "vitest";

import {
  INTERFACE_LANGUAGE_STORAGE_KEY,
  normalizeInterfaceLanguage,
  readSavedInterfaceLanguage,
} from "../src/lib/language";

describe("interface language", () => {
  /** 验证常见 BCP-47 变体映射到两套受支持资源。 */
  test("normalizes supported language families", () => {
    expect(normalizeInterfaceLanguage("zh-Hant-TW")).toBe("zh-CN");
    expect(normalizeInterfaceLanguage("en_GB")).toBe("en-US");
    expect(normalizeInterfaceLanguage("fr-FR")).toBe("en-US");
  });

  /** 验证只有合法的已保存偏好能够覆盖系统探测。 */
  test("accepts only a supported saved preference", () => {
    window.localStorage.setItem(INTERFACE_LANGUAGE_STORAGE_KEY, "zh-CN");
    expect(readSavedInterfaceLanguage()).toBe("zh-CN");
    window.localStorage.setItem(INTERFACE_LANGUAGE_STORAGE_KEY, "zh-TW");
    expect(readSavedInterfaceLanguage()).toBeUndefined();
  });
});
