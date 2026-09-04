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
    expect(screen.getByTestId("navigation-label-home")).toBeVisible();
    expect(screen.getByTestId("navigation-label-settings")).toBeVisible();
    expect(screen.queryByText(/sponsor/i)).not.toBeInTheDocument();
    expect(screen.queryByText(/about/i)).not.toBeInTheDocument();

    await userEvent.click(screen.getByRole("button", { name: "Settings" }));
    expect(onNavigate).toHaveBeenCalledWith("/settings");
  });
});
