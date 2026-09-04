import { MantineProvider, Text } from "@mantine/core";
import { IconLayoutDashboard, IconListCheck } from "@tabler/icons-react";
import "@testing-library/jest-dom/vitest";
import {
  cleanup,
  fireEvent,
  render,
  screen,
  within,
} from "@testing-library/react";
import { createInstance, type i18n } from "i18next";
import type { ReactElement } from "react";
import { I18nextProvider } from "react-i18next";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import enUS from "../i18n/en-US.json";
import zhCN from "../i18n/zh-CN.json";
import {
  APP_SIDEBAR_COLLAPSED_STORAGE_KEY,
  APP_SIDEBAR_COLLAPSE_ICON_SIZE_PX,
  APP_SIDEBAR_COMPACT_NAV_ITEM_GAP_PX,
  APP_SIDEBAR_COMPACT_NAV_ITEM_MIN_HEIGHT_PX,
  APP_SIDEBAR_COMPACT_NAV_ITEM_PADDING_BLOCK_PX,
  APP_SIDEBAR_COMPACT_PADDING_PX,
  APP_SIDEBAR_COMPACT_SECTION_GAP_PX,
  APP_SIDEBAR_DETAILED_NAV_ITEM_MIN_HEIGHT_PX,
  APP_SIDEBAR_ICON_STROKE_WIDTH,
  APP_SIDEBAR_LABEL_FONT_SIZE_PX,
  APP_SIDEBAR_LABEL_LINE_HEIGHT,
  APP_SIDEBAR_LOGO_SIZES,
  APP_SIDEBAR_NAV_ICON_SIZE_PX,
  APP_SIDEBAR_TOOLTIP_OPEN_DELAY_MS,
  APP_SIDEBAR_WIDTHS,
  DEFAULT_DETAILED_SIDEBAR_COLLAPSED,
  AppSidebarTemplate,
} from "./AppSidebarTemplate";
import { AppShellTemplate } from "./AppShellTemplate";
import {
  APP_THEME,
  APP_THEME_CSS_VARIABLES,
} from "./AppThemeProviderTemplate";
import { SponsorPageTemplate } from "./SponsorPageTemplate";
import { SupportMedia } from "./SupportMedia";
import { SettingsPageTemplate } from "./SettingsPageTemplate";
import {
  BRAND_SUPPORT_PROFILE,
  formatBrandWindowTitle,
  isLocalSupportPath,
  resolveBrandAssetPath,
} from "./brandSupportProfile";
import { formatDisplayVersion } from "./displayVersion";
import {
  MAX_VISIBLE_RELEASE_NOTE_ITEMS,
  MAX_VISIBLE_RELEASE_NOTE_VERSIONS,
} from "./releaseNotes";
import { buildSupportNavigationItems } from "./supportNavigation";

/** 为 jsdom 补齐 Mantine 布局组件依赖的只读观察器。 */
class TestResizeObserver implements ResizeObserver {
  /** 测试环境销毁观察器时不需要额外资源回收。 */
  disconnect(): void {}

  /** 测试环境只接受观察调用，不计算真实布局。 */
  observe(): void {}

  /** 测试环境允许组件停止观察指定元素。 */
  unobserve(): void {}
}

/** 创建只包含品牌支持 namespace 的真实 i18next 测试实例。 */
async function createTestI18n(locale: "zh-CN" | "en-US"): Promise<i18n> {
  const instance = createInstance();
  await instance.init({
    fallbackLng: "en-US",
    interpolation: { escapeValue: false },
    lng: locale,
    resources: {
      "en-US": { brandSupport: enUS },
      "zh-CN": { brandSupport: zhCN },
    },
  });
  return instance;
}

/** 使用真实 Mantine 与 i18next provider 渲染模板。 */
async function renderTemplate(
  node: ReactElement,
  locale: "zh-CN" | "en-US" = "zh-CN",
  colorScheme: "light" | "dark" = "light",
) {
  const instance = await createTestI18n(locale);
  return render(
    <I18nextProvider i18n={instance}>
      <MantineProvider forceColorScheme={colorScheme}>{node}</MantineProvider>
    </I18nextProvider>,
  );
}

describe("shared brand support templates", () => {
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

  /** 窗口标题始终只使用当前应用名和权威版本。 */
  it("formats the fixed dynamic window title from authoritative inputs", () => {
    expect(formatBrandWindowTitle("Example Utility", "3.4.5")).toBe(
      "Example Utility v3.4.5",
    );
    expect(formatBrandWindowTitle("Example Utility", "vv3.4.5")).toBe(
      "Example Utility v3.4.5",
    );
    expect(formatDisplayVersion("V3.4.5")).toBe("v3.4.5");
    expect(() => formatBrandWindowTitle(" ", "3.4.5")).toThrow(
      /application name and version/,
    );
    expect(zhCN.navigation).toEqual({
      settings: "设置",
      sponsor: "赞助",
    });
    expect(zhCN.tray).toEqual({ quit: "退出", show_window: "显示窗口" });
    expect(enUS.navigation).toEqual({
      settings: "Settings",
      sponsor: "Sponsor",
    });
    expect(enUS.tray).toEqual({ quit: "Quit", show_window: "Show Window" });
    expect(
      buildSupportNavigationItems({ sponsorPage: true }),
    ).toEqual([
      { id: "sponsor", labelKey: "navigation.sponsor", to: "/sponsor" },
      { id: "settings", labelKey: "navigation.settings", to: "/settings" },
    ]);
    expect(buildSupportNavigationItems({ sponsorPage: false })).toEqual([
      { id: "settings", labelKey: "navigation.settings", to: "/settings" },
    ]);
    expect(BRAND_SUPPORT_PROFILE.contacts).toEqual({
      support: { channel: "QQ", value: "2222980" },
    });
  });

  /** 共享品牌支持测试同时锚定系统通知与开机自启开关的固定契约名称。 */
  it("anchors the capability switch contract coverage in the shared support suite", async () => {
    const getSystemNotificationEnabled = vi.fn().mockResolvedValue(false);
    const getAutostartEnabled = vi.fn().mockResolvedValue(true);
    const capabilityContractNames = [
      "renders fixed controls without unselected capability or privacy sections",
      "system_notification_switch_uses_authoritative_success_result",
      "system_notification_switch_rolls_back_after_denial",
      "autostart_switch_rolls_back_after_failure",
    ] as const;

    await renderTemplate(
      <SettingsPageTemplate
        applicationName="Example Utility"
        autostart={{
          enabled: true,
          getEnabled: getAutostartEnabled,
          onChange: vi.fn().mockResolvedValue(true),
        }}
        language="zh-CN"
        onLanguageChange={vi.fn()}
        systemNotification={{
          enabled: false,
          getEnabled: getSystemNotificationEnabled,
          onChange: vi.fn().mockResolvedValue(false),
        }}
        version="3.4.5"
      />,
    );

    expect(capabilityContractNames).toHaveLength(4);
    expect(getSystemNotificationEnabled).not.toHaveBeenCalled();
    expect(getAutostartEnabled).not.toHaveBeenCalled();
    expect(
      screen.getByRole("switch", { name: "系统通知" }),
    ).toHaveAttribute("data-authoritative-state", "disabled");
    expect(
      screen.getByRole("switch", { name: "开机自启" }),
    ).toHaveAttribute("data-authoritative-state", "enabled");
  });

  /** 固定侧栏保持功能项向下增长，并把赞助、设置按固定顺序贴底。 */
  it("renders the fixed sidebar with a visible version and fixed bottom order", async () => {
    const onNavigate = vi.fn();
    await renderTemplate(
      <AppSidebarTemplate
        activePath="/overview"
        applicationName="Example Utility"
        featureItems={[
          {
            icon: IconLayoutDashboard,
            id: "overview",
            label: "总览",
            to: "/overview",
          },
          {
            icon: IconListCheck,
            id: "jobs",
            label: "任务",
            to: "/jobs",
          },
        ]}
        logoSrc="/app-identity/logo.png"
        mode="compact"
        onNavigate={onNavigate}
        supportPages={{ sponsorPage: true }}
        version="3.4.5"
      />,
    );

    const identity = screen.getByTestId("app-sidebar-identity");
    const logo = screen.getByRole("img", {
      name: "Example Utility 应用 Logo",
    });
    const version = screen.getByTestId("app-sidebar-version");
    expect(identity.firstElementChild).toBe(logo);
    expect(logo.nextElementSibling).toBe(version);
    expect(logo).toHaveAttribute("src", "/app-identity/logo.png");
    expect(screen.getByTestId("app-sidebar-version")).toHaveTextContent(
      "v3.4.5",
    );
    const overviewIcon = screen.getByTestId("navigation-icon-overview");
    expect(overviewIcon).toBeVisible();
    expect(overviewIcon).toHaveAttribute("width", "22");
    expect(overviewIcon).toHaveAttribute("height", "22");
    expect(overviewIcon).toHaveAttribute("stroke-width", "1.75");
    expect(
      within(screen.getByTestId("feature-navigation"))
        .getAllByRole("button")
        .map((item) => item.getAttribute("aria-label")),
    ).toEqual(["总览", "任务"]);
    expect(
      within(screen.getByTestId("fixed-bottom-navigation"))
        .getAllByRole("button")
        .map((item) => item.getAttribute("aria-label")),
    ).toEqual(["赞助", "设置"]);

    fireEvent.click(screen.getByRole("button", { name: "任务" }));
    expect(onNavigate).toHaveBeenCalledWith("/jobs");
  });

  /** 精简侧栏以图标在上、全宽居中文字在下，且没有展开控件。 */
  it("keeps compact icon-above-label navigation centered and non-expandable", async () => {
    expect(APP_SIDEBAR_WIDTHS.compact).toBe(80);
    expect(APP_SIDEBAR_LOGO_SIZES.compact).toBe(36);
    expect(APP_SIDEBAR_COMPACT_PADDING_PX).toBe(6);
    expect(APP_SIDEBAR_COMPACT_SECTION_GAP_PX).toBe(8);
    expect(APP_SIDEBAR_NAV_ICON_SIZE_PX).toBe(22);
    expect(APP_SIDEBAR_LABEL_FONT_SIZE_PX).toBe(11);
    expect(APP_SIDEBAR_LABEL_LINE_HEIGHT).toBe(1.25);
    expect(APP_SIDEBAR_COMPACT_NAV_ITEM_MIN_HEIGHT_PX).toBe(56);
    expect(APP_SIDEBAR_COMPACT_NAV_ITEM_PADDING_BLOCK_PX).toBe(4);
    expect(APP_SIDEBAR_COMPACT_NAV_ITEM_GAP_PX).toBe(4);
    await renderTemplate(
      <AppSidebarTemplate
        activePath="/settings"
        applicationName="Example Utility"
        featureItems={[
          {
            icon: IconLayoutDashboard,
            id: "overview",
            label: "一二三四五六七八九十",
            to: "/",
          },
        ]}
        logoSrc="/app-identity/logo.png"
        mode="compact"
        onNavigate={vi.fn()}
        supportPages={{ sponsorPage: true }}
        version="v9.8.7"
      />,
    );

    expect(screen.getByTestId("app-sidebar")).toHaveStyle({ width: "80px" });
    expect(screen.getByTestId("app-sidebar-content")).toHaveStyle({
      gap: "8px",
      padding: "6px",
    });
    expect(screen.getByTestId("app-sidebar")).toHaveAttribute(
      "data-layout",
      "compact",
    );
    expect(screen.getByTestId("app-sidebar-version")).toHaveTextContent(
      "v9.8.7",
    );
    expect(
      screen.getByRole("img", { name: "Example Utility 应用 Logo" }),
    ).toBeInTheDocument();
    expect(
      screen.queryByLabelText(/展开|收起|expand|collapse/i),
    ).not.toBeInTheDocument();
    expect(screen.getByRole("button", { name: "设置" })).toBeInTheDocument();
    expect(screen.getByTestId("app-sidebar-identity")).toHaveStyle({
      alignItems: "center",
      width: "100%",
    });
    for (const label of ["一二三四五六七八九十", "赞助", "设置"]) {
      const item = screen.getByRole("button", { name: label });
      expect(item).toHaveAttribute(
        "data-navigation-layout",
        "icon-above-label",
      );
      expect(item).toHaveAttribute("data-label-alignment", "full-width-center");
      expect(item).toHaveStyle({
        alignItems: "center",
        flexDirection: "column",
        gap: "4px",
        minHeight: "56px",
        paddingBlock: "4px",
        paddingInline: "0px",
      });
    }
    expect(screen.getByTestId("navigation-icon-overview")).toBeVisible();
    expect(screen.getByTestId("navigation-label-overview")).toHaveTextContent(
      "一二三四五六七八九十",
    );
    expect(screen.getByTestId("navigation-label-overview")).toHaveStyle({
      display: "block",
      fontSize: "11px",
      lineHeight: "1.25",
      marginInline: "auto",
      textAlign: "center",
      width: "100%",
    });
    expect(screen.getByTestId("navigation-label-overview")).not.toHaveStyle({
      inlineSize: "10em",
    });
  });

  /** 详细 AppShell 默认展开，并在按钮点击后同步侧栏、主区偏移和设备偏好。 */
  it("synchronizes the detailed AppShell width and restores its tooltip state", async () => {
    expect(DEFAULT_DETAILED_SIDEBAR_COLLAPSED).toBe(false);
    expect(APP_SIDEBAR_WIDTHS.detailedExpanded).toBe(248);
    expect(APP_SIDEBAR_WIDTHS.detailedCollapsed).toBe(76);
    expect(APP_SIDEBAR_LOGO_SIZES.detailedExpanded).toBe(72);
    expect(APP_SIDEBAR_LOGO_SIZES.detailedCollapsed).toBe(44);
    expect(APP_SIDEBAR_NAV_ICON_SIZE_PX).toBe(22);
    expect(APP_SIDEBAR_ICON_STROKE_WIDTH).toBe(1.75);
    expect(APP_SIDEBAR_DETAILED_NAV_ITEM_MIN_HEIGHT_PX).toBe(44);
    expect(APP_SIDEBAR_COLLAPSE_ICON_SIZE_PX).toBe(18);
    expect(APP_SIDEBAR_TOOLTIP_OPEN_DELAY_MS).toBe(0);
    window.localStorage.setItem(APP_SIDEBAR_COLLAPSED_STORAGE_KEY, "invalid");
    const shell = (
      <AppShellTemplate
        activePath="/overview"
        applicationName="Example Utility"
        featureItems={[
          {
            icon: IconLayoutDashboard,
            id: "overview",
            label: "总览",
            to: "/overview",
          },
        ]}
        onNavigate={vi.fn()}
        supportPages={{ sponsorPage: false }}
        version="1.2.3"
      >
        <Text>主内容</Text>
      </AppShellTemplate>
    );

    const firstRender = await renderTemplate(shell);
    expect(screen.getByTestId("app-shell")).toHaveAttribute(
      "data-mode",
      "detailed",
    );
    expect(screen.getByTestId("app-shell")).toHaveAttribute(
      "data-navbar-width",
      "248",
    );
    expect(screen.getByTestId("app-sidebar")).toHaveStyle({ width: "248px" });
    expect(screen.getByTestId("app-sidebar")).toHaveAttribute(
      "data-collapsed",
      "false",
    );
    expect(screen.getByTestId("navigation-label-overview")).toHaveTextContent(
      "总览",
    );
    expect(screen.getByRole("button", { name: "总览" })).toHaveAttribute(
      "data-navigation-layout",
      "icon-with-label",
    );
    expect(screen.getByRole("button", { name: "总览" })).toHaveStyle({
      flexDirection: "row",
      minHeight: "44px",
    });
    expect(screen.getByTestId("navigation-icon-overview")).toHaveAttribute(
      "width",
      "22",
    );
    expect(screen.getByTestId("app-sidebar-collapse-toggle")).toHaveAccessibleName(
      "收起侧栏",
    );

    fireEvent.click(screen.getByTestId("app-sidebar-identity"));
    expect(screen.getByTestId("app-shell")).toHaveAttribute(
      "data-navbar-width",
      "248",
    );
    expect(window.localStorage.getItem(APP_SIDEBAR_COLLAPSED_STORAGE_KEY)).toBe(
      "invalid",
    );

    fireEvent.click(screen.getByTestId("app-sidebar-collapse-toggle"));
    expect(window.localStorage.getItem(APP_SIDEBAR_COLLAPSED_STORAGE_KEY)).toBe(
      "true",
    );
    expect(screen.getByTestId("app-sidebar")).toHaveStyle({ width: "76px" });
    expect(screen.getByTestId("app-shell")).toHaveAttribute(
      "data-navbar-width",
      "76",
    );
    expect(screen.getByTestId("app-sidebar")).toHaveAttribute(
      "data-collapsed",
      "true",
    );
    expect(screen.queryByTestId("navigation-label-overview")).not.toBeInTheDocument();
    expect(screen.getByRole("button", { name: "总览" })).toHaveAttribute(
      "data-navigation-layout",
      "icon-only",
    );
    fireEvent.mouseEnter(screen.getByRole("button", { name: "总览" }));
    expect(await screen.findByText("总览")).toBeVisible();

    firstRender.unmount();
    await renderTemplate(shell);
    expect(screen.getByTestId("app-sidebar")).toHaveStyle({ width: "76px" });
    expect(screen.getByTestId("app-shell")).toHaveAttribute(
      "data-navbar-width",
      "76",
    );
    fireEvent.click(screen.getByTestId("app-sidebar-collapse-toggle"));
    expect(window.localStorage.getItem(APP_SIDEBAR_COLLAPSED_STORAGE_KEY)).toBe(
      "false",
    );
    expect(screen.getByTestId("app-sidebar")).toHaveStyle({ width: "248px" });
    expect(screen.getByTestId("app-shell")).toHaveAttribute(
      "data-navbar-width",
      "248",
    );
  });

  /** 初始化主题同时提供可区分的亮色与暗色背景、文字和表面令牌。 */
  it("defines distinct light and dark application theme variables", () => {
    const variables = APP_THEME_CSS_VARIABLES(APP_THEME);
    for (const name of [
      "--app-accent",
      "--app-background",
      "--app-border",
      "--app-surface",
      "--app-text",
      "--app-text-muted",
    ]) {
      expect(variables.light?.[name]).not.toBe(variables.dark?.[name]);
    }
  });

  /** 设置页固定承接当前应用名称与权威版本，不再展示标题联系人。 */
  it("renders application identity and version in the fixed settings card", async () => {
    await renderTemplate(
      <SettingsPageTemplate
        applicationName="Example Utility"
        language="zh-CN"
        onLanguageChange={vi.fn()}
        version="v3.4.5"
      />,
    );

    expect(screen.getByTestId("settings-application-section")).toBeVisible();
    expect(
      screen.getByRole("heading", { name: "应用信息" }),
    ).toBeInTheDocument();
    expect(screen.getByText("Example Utility")).toBeInTheDocument();
    expect(screen.getByText("版本 v3.4.5")).toBeInTheDocument();
    expect(screen.queryByText(/vv3\.4\.5/)).not.toBeInTheDocument();
    expect(screen.queryByText(/2222980/)).not.toBeInTheDocument();
    expect(screen.queryByText("免责声明")).not.toBeInTheDocument();
  });

  /** 发布日志区容器不代理按钮动作，并严格裁剪到五版和每类十条。 */
  it("keeps release-note actions bound to their own controls and limits entries", async () => {
    const releases = Array.from({ length: 6 }, (_, releaseIndex) => {
      const sequence = 6 - releaseIndex;
      return {
        bugFixes: [
          {
            "en-US": `Version ${sequence} fix`,
            "zh-CN": `版本 ${sequence} 修复`,
          },
        ],
        featureOptimizations: Array.from(
          { length: 11 },
          (_, itemIndex) => ({
            "en-US": `Version ${sequence} improvement ${itemIndex + 1}`,
            "zh-CN": `版本 ${sequence} 优化 ${itemIndex + 1}`,
          }),
        ),
        releaseDate: `2026-08-${20 + sequence}`,
        version: sequence === 6 ? `v1.0.${sequence}` : `1.0.${sequence}`,
      };
    });
    await renderTemplate(
      <SettingsPageTemplate
        applicationName="Example Utility"
        language="zh-CN"
        onLanguageChange={vi.fn()}
        releaseNotesLoader={async () => releases}
        version="1.0.6"
      />,
    );

    expect(MAX_VISIBLE_RELEASE_NOTE_VERSIONS).toBe(5);
    expect(MAX_VISIBLE_RELEASE_NOTE_ITEMS).toBe(10);
    fireEvent.click(screen.getByTestId("settings-release-notes-section"));
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument();

    fireEvent.click(screen.getByRole("button", { name: "查看更新日志" }));
    expect(screen.getByRole("dialog", { name: "更新日志" })).toBeInTheDocument();
    expect(
      await screen.findByText(
        "-----------更新日志 2026-08-26 v1.0.6----------",
      ),
    ).toBeInTheDocument();
    expect(screen.getAllByText("###功能优化")).toHaveLength(5);
    expect(screen.getAllByText("###问题修复")).toHaveLength(5);
    expect(screen.getByText("版本 6 优化 10")).toBeInTheDocument();
    expect(screen.queryByText("版本 6 优化 11")).not.toBeInTheDocument();
    expect(screen.queryByText(/v1\.0\.1/)).not.toBeInTheDocument();
    expect(screen.queryByText(/vv1\.0\.6/)).not.toBeInTheDocument();
  });

  /** 英文界面必须选择同一发布事实的 en-US 文案与英文标题。 */
  it("selects English release-note translations from the active locale", async () => {
    const releases = [
      {
        bugFixes: [{ "en-US": "Fix startup", "zh-CN": "修复启动问题" }],
        featureOptimizations: [
          { "en-US": "Add export", "zh-CN": "新增导出能力" },
        ],
        releaseDate: "2026-08-28",
        version: "1.2.3",
      },
    ];
    await renderTemplate(
      <SettingsPageTemplate
        applicationName="Example Utility"
        language="en-US"
        onLanguageChange={vi.fn()}
        releaseNotesLoader={async () => releases}
        version="1.2.3"
      />,
      "en-US",
    );

    fireEvent.click(
      screen.getByRole("button", { name: "View release notes" }),
    );
    expect(
      await screen.findByText(
        "-----------Release notes 2026-08-28 v1.2.3----------",
      ),
    ).toBeInTheDocument();
    expect(screen.getByText("###Feature optimizations")).toBeInTheDocument();
    expect(screen.getByText("###Bug fixes")).toBeInTheDocument();
    expect(screen.getByText("Add export")).toBeInTheDocument();
    expect(screen.getByText("Fix startup")).toBeInTheDocument();
    expect(screen.queryByText("新增导出能力")).not.toBeInTheDocument();
  });

  /** 候选资源读取失败时展示本地错误，并允许用户从按钮自身重试。 */
  it("shows a bounded release notes load failure and retries from its own control", async () => {
    const releases = [
      {
        bugFixes: [
          {
            "en-US": "Fix bundled resource loading",
            "zh-CN": "修复候选资源读取",
          },
        ],
        featureOptimizations: [],
        releaseDate: "2026-08-27",
        version: "v1.0.7",
      },
    ];
    const releaseNotesLoader = vi
      .fn(async () => releases)
      .mockRejectedValueOnce(new Error("unavailable"));
    await renderTemplate(
      <SettingsPageTemplate
        applicationName="Example Utility"
        language="zh-CN"
        onLanguageChange={vi.fn()}
        releaseNotesLoader={releaseNotesLoader}
        version="1.0.7"
      />,
    );

    fireEvent.click(screen.getByRole("button", { name: "查看更新日志" }));
    expect(await screen.findByRole("alert")).toHaveTextContent(
      "无法读取此候选内的更新日志",
    );
    fireEvent.click(screen.getByRole("button", { name: "重试" }));
    expect(
      await screen.findByText(
        "-----------更新日志 2026-08-27 v1.0.7----------",
      ),
    ).toBeInTheDocument();
    expect(releaseNotesLoader).toHaveBeenCalledTimes(2);
  });

  /** 设置页固定提供本地发布日志入口，不恢复联网检查更新动作。 */
  it("keeps only the local release-notes action on Settings", async () => {
    await renderTemplate(
      <SettingsPageTemplate
        applicationName="Example Utility"
        language="en-US"
        onLanguageChange={vi.fn()}
        version="1.0.0"
      />,
      "en-US",
    );

    expect(
      screen.getByRole("button", { name: "View release notes" }),
    ).toBeEnabled();
    expect(
      screen.queryByRole("button", { name: /check for updates/i }),
    ).not.toBeInTheDocument();
    expect(screen.queryByText(/QQ:2222980/)).not.toBeInTheDocument();
  });

  /** 赞助页展示三档固定价格、品牌联系人、档位插图和两种支付码。 */
  it("renders the complete fixed sponsor profile with accessible payment images", async () => {
    await renderTemplate(<SponsorPageTemplate />);

    expect(screen.getByText("19")).toBeInTheDocument();
    expect(screen.getByText("199")).toBeInTheDocument();
    expect(screen.getByText("1999")).toBeInTheDocument();
    expect(screen.getAllByText(/2222980/).length).toBeGreaterThan(0);
    expect(
      screen.getByRole("img", { name: "微信支付赞助二维码" }),
    ).toBeInTheDocument();
    expect(
      screen.getByRole("img", { name: "支付宝赞助二维码" }),
    ).toBeInTheDocument();
    expect(screen.getAllByRole("article")).toHaveLength(3);
    const pageStyle =
      screen.getByTestId("brand-sponsor-page").getAttribute("style") ?? "";
    expect(screen.getByTestId("brand-sponsor-page")).toHaveAttribute(
      "data-color-scheme",
      "light",
    );
    expect(pageStyle).not.toMatch(/min-width|pointer-events/i);
  });

  /** 英文深色方案复用同一品牌事实，并保持支付码可访问名称。 */
  it("renders the same sponsor profile in English and dark color scheme", async () => {
    await renderTemplate(<SponsorPageTemplate />, "en-US", "dark");

    expect(screen.getByTestId("brand-sponsor-page")).toHaveAttribute(
      "data-color-scheme",
      "dark",
    );
    expect(screen.getAllByRole("article")).toHaveLength(3);
    expect(
      screen
        .getAllByRole("article")
        .every((card) => card.getAttribute("data-color-scheme") === "dark"),
    ).toBe(true);
    expect(
      screen.getByRole("heading", {
        name: "Freely Maintained by Shoucheng & Feiying Studio",
      }),
    ).toBeInTheDocument();
    expect(screen.getAllByText("CNY")).toHaveLength(3);
    expect(
      screen.getByRole("img", { name: "WeChat Pay sponsorship QR code" }),
    ).toBeInTheDocument();
    expect(screen.getByText("QQ 2222980")).toBeInTheDocument();
  });

  /** 本地视频始终带 controls、字幕和文字稿，且不会自动播放。 */
  it("renders a local video with captions and transcript without autoplay", async () => {
    await renderTemplate(
      <SupportMedia
        media={{
          captionsLabel: "简体中文字幕",
          captionsLanguage: "zh-CN",
          captionsSrc: "/brand-support/video/demo.zh-CN.vtt",
          kind: "video",
          poster: "/brand-support/video/demo.jpg",
          src: "/brand-support/video/demo.webm",
          title: "功能演示",
          transcriptHref: "/brand-support/video/demo.zh-CN.txt",
          transcriptLabel: "查看文字稿",
        }}
      />,
    );

    const video = screen.getByLabelText("功能演示") as HTMLVideoElement;
    expect(video.controls).toBe(true);
    expect(video.autoplay).toBe(false);
    expect(video.querySelector('track[kind="captions"]')).toHaveAttribute(
      "src",
      "/brand-support/video/demo.zh-CN.vtt",
    );
    expect(screen.getByRole("link", { name: "查看文字稿" })).toHaveAttribute(
      "href",
      "/brand-support/video/demo.zh-CN.txt",
    );
  });

  /** 远程媒体与路径跳转不能绕过本地打包边界。 */
  it("rejects remote or escaping media paths", () => {
    expect(isLocalSupportPath("https://media.invalid/demo.mp4")).toBe(false);
    expect(isLocalSupportPath("/brand-support/../secret")).toBe(false);
    expect(() =>
      resolveBrandAssetPath("/brand-support", "../secret"),
    ).toThrow();
  });

  /** 可选品牌资源只包含 sponsor 辅助图片。 */
  it("keeps the six optional sponsor assets", () => {
    expect(BRAND_SUPPORT_PROFILE.optionalAssets).toHaveLength(6);
  });
});
