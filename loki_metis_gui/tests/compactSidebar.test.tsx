import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, test, vi } from "vitest";

import {
  APP_SIDEBAR_LOGO_SIZES,
  APP_SIDEBAR_NAV_ITEM_MIN_HEIGHT_PX,
  APP_SIDEBAR_NAV_ICON_SIZE_PX,
  APP_SIDEBAR_WIDTHS,
  COMPACT_PADDING,
  AppSidebar,
} from "../src/components/AppSidebar";
import { appI18n } from "../src/i18n";
import { TestProviders } from "./testUtils";

describe("compact application sidebar", () => {
  /** 锁定 compact 标准像素、持续标签和未选择支持页的零入口。 */
  test("uses the approved compact geometry and navigation", async () => {
    const onNavigate = vi.fn();
    render(
      <TestProviders>
        <AppSidebar
          activePath="/"
          applicationName="LokiMetis"
          onNavigate={onNavigate}
          version="0.1.0"
        />
      </TestProviders>,
    );

    expect(APP_SIDEBAR_WIDTHS.compact).toBe(80);
    expect(COMPACT_PADDING).toBe(6);
    expect(APP_SIDEBAR_LOGO_SIZES.compact).toBe(36);
    expect(APP_SIDEBAR_NAV_ICON_SIZE_PX).toBe(22);
    expect(APP_SIDEBAR_NAV_ITEM_MIN_HEIGHT_PX).toBe(56);
    expect(screen.getByTestId("app-sidebar")).toHaveAttribute("data-mode", "compact");
    expect(screen.queryByTestId("navigation-label-home")).not.toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Home" })).not.toBeInTheDocument();
    expect(screen.getByTestId("navigation-dashboard")).toBeVisible();
    expect(screen.getByRole("button", { name: "Usage Dashboard" })).toHaveAttribute(
      "data-active",
      "true",
    );
    expect(screen.getByRole("button", { name: "AI Monitor" })).not.toHaveAttribute(
      "data-active",
    );
    expect(screen.getByTestId("navigation-monitor")).toBeVisible();
    expect(screen.getByTestId("navigation-skins")).toBeVisible();
    expect(screen.getByTestId("navigation-settings")).toBeVisible();
    expect(screen.queryByRole("button", { name: "Workbench" })).not.toBeInTheDocument();
    expect(screen.queryByTestId("open-pet-overlay")).not.toBeInTheDocument();
    expect(screen.queryByRole("button", { name: /Desktop pet/i })).not.toBeInTheDocument();
    expect(
      screen.queryByRole("button", { name: "Monitor management" }),
    ).not.toBeInTheDocument();
    expect(
      screen.queryByRole("button", { name: "Image management" }),
    ).not.toBeInTheDocument();
    expect(screen.queryByText(/sponsor/i)).not.toBeInTheDocument();
    expect(screen.queryByText(/about/i)).not.toBeInTheDocument();
    expect(screen.queryByText(/charts/i)).not.toBeInTheDocument();
    expect(screen.queryByText(/leaderboard/i)).not.toBeInTheDocument();
    expect(screen.queryByText(/data collection/i)).not.toBeInTheDocument();

    await userEvent.click(screen.getByRole("button", { name: "Usage Dashboard" }));
    expect(onNavigate).toHaveBeenCalledWith("/dashboard");
    await userEvent.click(screen.getByRole("button", { name: "AI Monitor" }));
    expect(onNavigate).toHaveBeenCalledWith("/monitor");
    await userEvent.click(screen.getByRole("button", { name: "App Skins" }));
    expect(onNavigate).toHaveBeenCalledWith("/skins");
    expect(onNavigate).not.toHaveBeenCalledWith("/settings");
    await userEvent.click(screen.getByRole("button", { name: "Settings" }));
    expect(onNavigate).toHaveBeenCalledWith("/settings");
  });

  /** 中文侧栏使用已批准的“应用换肤”，同时保持原有路由和点击所有权。 */
  test("uses the approved Chinese skin navigation label", async () => {
    await appI18n.changeLanguage("zh-CN");
    const onNavigate = vi.fn();
    render(
      <TestProviders>
        <AppSidebar
          activePath="/skins"
          applicationName="LokiMetis"
          onNavigate={onNavigate}
          version="0.2.16"
        />
      </TestProviders>,
    );

    const skinsNavigation = screen.getByRole("button", { name: "应用换肤" });
    expect(skinsNavigation).toHaveAttribute("data-active", "true");
    expect(screen.queryByRole("button", { name: "应用换皮" })).not.toBeInTheDocument();
    await userEvent.click(skinsNavigation);
    expect(onNavigate).toHaveBeenCalledWith("/skins");
  });
});
