import { MantineProvider } from "@mantine/core";
import "@testing-library/jest-dom/vitest";
import {
  cleanup,
  fireEvent,
  render,
  screen,
  waitFor,
} from "@testing-library/react";
import { createInstance, type i18n } from "i18next";
import type { ReactElement } from "react";
import { I18nextProvider } from "react-i18next";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import enUS from "../i18n/en-US.json";
import zhCN from "../i18n/zh-CN.json";
import {
  APP_COLOR_SCHEME_STORAGE_KEY,
  AppThemeProviderTemplate,
} from "./AppThemeProviderTemplate";
import { SettingsPageTemplate } from "./SettingsPageTemplate";

/** 为 jsdom 补齐 Mantine 布局组件依赖的只读观察器。 */
class TestResizeObserver implements ResizeObserver {
  /** 测试环境销毁观察器时不需要额外资源回收。 */
  disconnect(): void {}

  /** 测试环境只接受观察调用，不计算真实布局。 */
  observe(): void {}

  /** 测试环境允许组件停止观察指定元素。 */
  unobserve(): void {}
}

/** 创建只包含 GUI 支持 namespace 的真实 i18next 测试实例。 */
async function createTestI18n(locale: "zh-CN" | "en-US"): Promise<i18n> {
  const instance = createInstance();
  await instance.init({
    fallbackLng: "en-US",
    interpolation: { escapeValue: false },
    lng: locale,
    resources: {
      "en-US": { guiSupport: enUS },
      "zh-CN": { guiSupport: zhCN },
    },
  });
  return instance;
}

/** 使用真实应用主题 provider 验证设置页设备级偏好与宿主能力状态。 */
async function renderAppThemeTemplate(node: ReactElement) {
  const instance = await createTestI18n("zh-CN");
  return render(
    <I18nextProvider i18n={instance}>
      <MantineProvider forceColorScheme="light">
        <AppThemeProviderTemplate>{node}</AppThemeProviderTemplate>
      </MantineProvider>
    </I18nextProvider>,
  );
}

/** 创建可由测试精确推进的 Promise。 */
function createDeferred<T>(): {
  promise: Promise<T>;
  reject: (reason?: unknown) => void;
  resolve: (value: T) => void;
} {
  let rejectPromise: (reason?: unknown) => void = () => undefined;
  let resolvePromise: (value: T) => void = () => undefined;
  const promise = new Promise<T>((resolve, reject) => {
    rejectPromise = reject;
    resolvePromise = resolve;
  });
  return { promise, reject: rejectPromise, resolve: resolvePromise };
}

describe("capability switch settings template", () => {
  beforeEach(() => {
    window.localStorage.clear();
    Object.defineProperty(globalThis, "ResizeObserver", {
      configurable: true,
      value: TestResizeObserver,
    });
    Object.defineProperty(window, "matchMedia", {
      configurable: true,
      value: vi.fn().mockImplementation((query: string) => ({
        addEventListener: vi.fn(),
        addListener: vi.fn(),
        dispatchEvent: vi.fn(),
        matches: false,
        media: query,
        onchange: null,
        removeEventListener: vi.fn(),
        removeListener: vi.fn(),
      })),
    });
  });

  afterEach(() => {
    cleanup();
  });

  /** 未选择宿主能力时，设置页仍固定显示应用、版本、发布说明、语言和主题。 */
  it("renders fixed controls without unselected capability or privacy sections", async () => {
    const onLanguageChange = vi.fn();
    const releaseNotesLoader = vi.fn().mockResolvedValue([]);
    await renderAppThemeTemplate(
      <SettingsPageTemplate
        applicationName="Example Utility"
        language="zh-CN"
        onLanguageChange={onLanguageChange}
        releaseNotesLoader={releaseNotesLoader}
        version="3.4.5"
      />,
    );

    expect(screen.getByText("Example Utility")).toBeInTheDocument();
    expect(screen.getByText("版本 v3.4.5")).toBeInTheDocument();
    expect(
      screen.getByRole("button", { name: "查看更新日志" }),
    ).toBeEnabled();
    expect(releaseNotesLoader).not.toHaveBeenCalled();
    fireEvent.click(screen.getByRole("radio", { name: "English" }));
    fireEvent.click(screen.getByRole("radio", { name: "深色" }));
    expect(window.localStorage.getItem(APP_COLOR_SCHEME_STORAGE_KEY)).toBe(
      "dark",
    );
    expect(screen.getByRole("radio", { name: "深色" })).toBeChecked();
    expect(screen.getByTestId("app-theme-surface")).toHaveAttribute(
      "data-color-scheme",
      "dark",
    );
    fireEvent.click(screen.getByRole("radio", { name: "跟随系统" }));
    expect(onLanguageChange).toHaveBeenCalledWith("en-US");
    expect(window.localStorage.getItem(APP_COLOR_SCHEME_STORAGE_KEY)).toBe(
      "auto",
    );
    expect(screen.getByRole("radio", { name: "跟随系统" })).toBeChecked();
    expect(screen.getByTestId("app-theme-surface")).toHaveAttribute(
      "data-color-scheme",
      "light",
    );
    expect(screen.queryByRole("switch")).not.toBeInTheDocument();
    expect(screen.queryByText("隐私")).not.toBeInTheDocument();
    expect(screen.queryByText(/统计/)).not.toBeInTheDocument();
  });

  /** 系统通知只由 Switch 自身提交，并采用命令返回的最终持久化状态。 */
  it("system_notification_switch_uses_authoritative_success_result", async () => {
    const notificationRequest = createDeferred<boolean>();
    const onSystemNotificationChange = vi.fn(() => notificationRequest.promise);
    await renderAppThemeTemplate(
      <SettingsPageTemplate
        applicationName="Example Utility"
        language="zh-CN"
        onLanguageChange={vi.fn()}
        systemNotification={{
          enabled: false,
          getEnabled: vi.fn().mockResolvedValue(false),
          onChange: onSystemNotificationChange,
        }}
        version="3.4.5"
      />,
    );

    const notificationTitle = screen.getByTestId(
      "settings-capability-title-system_notification",
    );
    const notificationDescription = screen.getByTestId(
      "settings-capability-description-system_notification",
    );
    const notificationSwitch = screen.getByRole("switch", {
      name: "系统通知",
    });
    expect(notificationTitle).toHaveAttribute(
      "id",
      "settings-capability-title-system_notification",
    );
    expect(notificationDescription).toHaveAttribute(
      "id",
      "settings-capability-description-system_notification",
    );
    expect(notificationSwitch).toHaveAttribute(
      "aria-labelledby",
      notificationTitle.id,
    );
    expect(notificationSwitch).toHaveAttribute(
      "aria-describedby",
      notificationDescription.id,
    );

    fireEvent.click(
      screen.getByTestId("settings-capability-system_notification"),
    );
    expect(onSystemNotificationChange).not.toHaveBeenCalled();
    fireEvent.click(notificationTitle);
    fireEvent.click(notificationDescription);
    expect(onSystemNotificationChange).not.toHaveBeenCalled();

    fireEvent.click(notificationSwitch);
    expect(onSystemNotificationChange).toHaveBeenCalledWith(true);
    expect(notificationSwitch).toBeDisabled();
    expect(screen.getByRole("status")).toHaveTextContent("正在更新系统设置");
    notificationRequest.resolve(true);
    await waitFor(() => expect(notificationSwitch).toBeChecked());
    expect(screen.getByRole("status")).toHaveTextContent("已开启");
  });

  /** 系统权限拒绝后通过 get command 恢复真实偏好并显示可感知错误。 */
  it("system_notification_switch_rolls_back_after_denial", async () => {
    const getSystemNotificationEnabled = vi.fn().mockResolvedValue(false);
    const onSystemNotificationChange = vi
      .fn()
      .mockRejectedValue(new Error("permission-denied"));
    await renderAppThemeTemplate(
      <SettingsPageTemplate
        applicationName="Example Utility"
        language="zh-CN"
        onLanguageChange={vi.fn()}
        systemNotification={{
          enabled: false,
          getEnabled: getSystemNotificationEnabled,
          onChange: onSystemNotificationChange,
        }}
        version="3.4.5"
      />,
    );

    const notificationSwitch = screen.getByRole("switch", {
      name: "系统通知",
    });
    fireEvent.click(notificationSwitch);
    await waitFor(() =>
      expect(onSystemNotificationChange).toHaveBeenCalledWith(true),
    );
    await waitFor(() =>
      expect(getSystemNotificationEnabled).toHaveBeenCalledOnce(),
    );
    await waitFor(() => expect(notificationSwitch).not.toBeChecked());
    expect(screen.getByRole("alert")).toHaveTextContent("无法完成通知设置更新");
  });

  /** mutation 与重读都失败时，系统通知必须进入未知态而不是回退旧值。 */
  it("system_notification_switch_enters_unknown_state_when_reread_fails", async () => {
    const unknownStateMessage = "actual system state is currently unavailable";
    const getSystemNotificationEnabled = vi
      .fn()
      .mockRejectedValue(new Error("status-unavailable"));
    const onSystemNotificationChange = vi
      .fn()
      .mockRejectedValue(new Error("permission-denied"));
    await renderAppThemeTemplate(
      <SettingsPageTemplate
        applicationName="Example Utility"
        language="zh-CN"
        onLanguageChange={vi.fn()}
        systemNotification={{
          enabled: true,
          getEnabled: getSystemNotificationEnabled,
          onChange: onSystemNotificationChange,
        }}
        version="3.4.5"
      />,
    );

    const notificationSwitch = screen.getByRole("switch", {
      name: "系统通知",
    });
    expect(notificationSwitch).toBeChecked();
    fireEvent.click(notificationSwitch);
    await waitFor(() =>
      expect(onSystemNotificationChange).toHaveBeenCalledWith(false),
    );
    await waitFor(() =>
      expect(getSystemNotificationEnabled).toHaveBeenCalledOnce(),
    );
    await waitFor(() =>
      expect(notificationSwitch).toHaveAttribute(
        "data-authoritative-state",
        "unknown",
      ),
    );
    expect(notificationSwitch).toBeDisabled();
    expect(screen.getByRole("alert")).toHaveTextContent("当前状态未知");
    expect(screen.getByRole("alert")).toHaveTextContent("暂时无法确认");
    expect(unknownStateMessage).toContain("currently unavailable");
    expect(
      screen.getByRole("button", { name: "重新读取实际状态" }),
    ).toBeInTheDocument();
  });

  /** 自启 mutation 失败后重读 OS；实际状态即使不同于旧 UI 也必须覆盖旧值。 */
  it("autostart_switch_rolls_back_after_failure", async () => {
    const getAutostartEnabled = vi.fn().mockResolvedValue(true);
    const onAutostartChange = vi.fn().mockRejectedValue(new Error("denied"));
    await renderAppThemeTemplate(
      <SettingsPageTemplate
        applicationName="Example Utility"
        autostart={{
          enabled: false,
          getEnabled: getAutostartEnabled,
          onChange: onAutostartChange,
        }}
        language="zh-CN"
        onLanguageChange={vi.fn()}
        version="3.4.5"
      />,
    );

    const autostartTitle = screen.getByTestId(
      "settings-capability-title-autostart",
    );
    const autostartDescription = screen.getByTestId(
      "settings-capability-description-autostart",
    );
    const autostartSwitch = screen.getByRole("switch", { name: "开机自启" });
    expect(autostartSwitch).toHaveAttribute(
      "aria-labelledby",
      autostartTitle.id,
    );
    expect(autostartSwitch).toHaveAttribute(
      "aria-describedby",
      autostartDescription.id,
    );

    fireEvent.click(screen.getByTestId("settings-capability-autostart"));
    expect(onAutostartChange).not.toHaveBeenCalled();
    fireEvent.click(autostartTitle);
    fireEvent.click(autostartDescription);
    expect(onAutostartChange).not.toHaveBeenCalled();
    fireEvent.click(autostartSwitch);
    await waitFor(() => expect(onAutostartChange).toHaveBeenCalledWith(true));
    await waitFor(() => expect(getAutostartEnabled).toHaveBeenCalledOnce());
    await waitFor(() => expect(autostartSwitch).toBeChecked());
    expect(screen.getByRole("alert")).toHaveTextContent("无法完成开机自启更新");
  });

  /** mutation 与重读都失败时，开机自启必须进入未知态并提供重试入口。 */
  it("autostart_switch_enters_unknown_state_when_reread_fails", async () => {
    const getAutostartEnabled = vi
      .fn()
      .mockRejectedValue(new Error("status-unavailable"));
    const onAutostartChange = vi.fn().mockRejectedValue(new Error("denied"));
    await renderAppThemeTemplate(
      <SettingsPageTemplate
        applicationName="Example Utility"
        autostart={{
          enabled: false,
          getEnabled: getAutostartEnabled,
          onChange: onAutostartChange,
        }}
        language="zh-CN"
        onLanguageChange={vi.fn()}
        version="3.4.5"
      />,
    );

    const autostartSwitch = screen.getByRole("switch", { name: "开机自启" });
    fireEvent.click(autostartSwitch);
    await waitFor(() => expect(onAutostartChange).toHaveBeenCalledWith(true));
    await waitFor(() => expect(getAutostartEnabled).toHaveBeenCalledOnce());
    await waitFor(() =>
      expect(autostartSwitch).toHaveAttribute(
        "data-authoritative-state",
        "unknown",
      ),
    );
    expect(autostartSwitch).toBeDisabled();
    expect(screen.getByRole("alert")).toHaveTextContent("当前状态未知");
    expect(screen.getByRole("alert")).toHaveTextContent("无法读取操作系统登录项");

    getAutostartEnabled.mockResolvedValueOnce(true);
    fireEvent.click(screen.getByRole("button", { name: "重新读取实际状态" }));
    await waitFor(() => expect(getAutostartEnabled).toHaveBeenCalledTimes(2));
    await waitFor(() => expect(autostartSwitch).toBeChecked());
    await waitFor(() =>
      expect(autostartSwitch).toHaveAttribute(
        "data-authoritative-state",
        "enabled",
      ),
    );
    expect(screen.getByRole("status")).toHaveTextContent("已开启");
    expect(
      screen.queryByRole("button", { name: "重新读取实际状态" }),
    ).not.toBeInTheDocument();
  });
});
