import { describe, expect, test } from "vitest";

import { appI18n } from "../src/i18n";
import { visibleErrorMessage } from "../src/visible-error";

describe("visibleErrorMessage", () => {
  /** 白名单 Rust 错误按当前语言显示稳定原因，不退化为对象字符串。 */
  test("localizes_a_known_structured_rust_error", async () => {
    await appI18n.changeLanguage("zh-CN");

    expect(
      visibleErrorMessage({
        code: "error.hooks.directoryNotAbsolute",
        params: { path: "relative/hooks" },
      }),
    ).toBe("Hooks 配置目录必须使用绝对路径。");
  });

  /** 后端 detail/path 等诊断参数可能含用户路径，界面不得直接渲染。 */
  test("does_not_render_sensitive_structured_error_params", () => {
    const message = visibleErrorMessage({
      code: "error.hooks.writeFailed",
      params: {
        detail: "permission denied at /Users/private/account",
        path: "/Users/private/account/hooks.json",
      },
    });

    expect(message).toBe(
      "The Hooks configuration could not be written. Check the selected path and its permissions.",
    );
    expect(message).not.toContain("/Users/private");
    expect(message).not.toContain("permission denied");
  });

  /** 未登记对象与普通运行时错误继续使用固定安全兜底，不泄露原始内容。 */
  test("uses_the_safe_fallback_for_unknown_errors", () => {
    expect(
      visibleErrorMessage({
        code: "error.internal.secret",
        params: { detail: "/private/path" },
      }),
    ).toBe("The operation could not be completed. Try again.");
    expect(visibleErrorMessage(new Error("private stack detail"))).toBe(
      "The operation could not be completed. Try again.",
    );
  });
});
