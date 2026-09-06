import { Box, Divider, Image, NavLink, ScrollArea, Stack, Text } from "@mantine/core";
import {
  IconDeviceDesktopAnalytics,
  IconLayoutDashboard,
  IconSettings,
  type TablerIcon,
} from "@tabler/icons-react";
import type { ReactElement } from "react";
import { useTranslation } from "react-i18next";

import { isDashboardLandingPath } from "../default-landing";
import { formatDisplayVersion } from "../lib/releaseNotes";

export const sidebarMode = "compact";
export const APP_SIDEBAR_WIDTHS = { compact: 80 } as const;
export const APP_SIDEBAR_LOGO_SIZES = { compact: 36 } as const;
export const COMPACT_PADDING = 6;
export const SECTION_GAP = 8;
export const APP_SIDEBAR_NAV_ICON_SIZE_PX = 22;
export const APP_SIDEBAR_ICON_STROKE_WIDTH = 1.75;
export const APP_SIDEBAR_LABEL_FONT_SIZE_PX = 11;
export const APP_SIDEBAR_LABEL_LINE_HEIGHT = 1.25;
export const APP_SIDEBAR_NAV_ITEM_MIN_HEIGHT_PX = 56;
export const APP_SIDEBAR_NAV_ITEM_PADDING_BLOCK_PX = 4;
export const APP_SIDEBAR_NAV_ITEM_GAP_PX = 4;
export const APP_SIDEBAR_LOGO_PATH = "/app-identity/logo.png";
export const APP_SIDEBAR_COMPACT_PADDING_PX = COMPACT_PADDING;
export const APP_SIDEBAR_COMPACT_NAV_ITEM_MIN_HEIGHT_PX =
  APP_SIDEBAR_NAV_ITEM_MIN_HEIGHT_PX;

/** 描述精简侧栏中的单个导航目的地。 */
interface NavigationItem {
  id: "dashboard" | "monitor" | "settings";
  label: string;
  path: "/dashboard" | "/monitor" | "/settings";
  icon: TablerIcon;
}

/** 描述精简侧栏所需的安装包事实和导航回调。 */
export interface AppSidebarProps {
  activePath: string;
  applicationName: string;
  mode?: "compact";
  version: string;
  onNavigate: (path: "/dashboard" | "/monitor" | "/settings") => void;
}

/** 渲染图标在上且标签持续可见的精简导航项。 */
function AppSidebarNavigationItem({
  active,
  item,
  onNavigate,
}: {
  active: boolean;
  item: NavigationItem;
  onNavigate: (path: "/dashboard" | "/monitor" | "/settings") => void;
}): ReactElement {
  const Icon = item.icon;
  return (
    <NavLink
      active={active}
      aria-label={item.label}
      component="button"
      data-navigation-layout="icon-above-label"
      label={
        <Text
          data-testid={`navigation-label-${item.id}`}
          lineClamp={2}
          style={{
            display: "block",
            fontSize: APP_SIDEBAR_LABEL_FONT_SIZE_PX,
            lineHeight: APP_SIDEBAR_LABEL_LINE_HEIGHT,
            marginInline: "auto",
            overflowWrap: "anywhere",
            textAlign: "center",
            width: "100%",
          }}
        >
          {item.label}
        </Text>
      }
      leftSection={
        <Icon
          aria-hidden="true"
          data-testid={`navigation-icon-${item.id}`}
          size={APP_SIDEBAR_NAV_ICON_SIZE_PX}
          stroke={APP_SIDEBAR_ICON_STROKE_WIDTH}
        />
      }
      onClick={() => {
        onNavigate(item.path);
      }}
      styles={{
        body: {
          flex: "0 0 auto",
          overflow: "visible",
          textAlign: "center",
          width: "100%",
        },
        label: {
          display: "block",
          marginInline: "auto",
          textAlign: "center",
          whiteSpace: "normal",
          width: "100%",
        },
        root: {
          alignItems: "center",
          borderRadius: 12,
          flexDirection: "column",
          gap: APP_SIDEBAR_NAV_ITEM_GAP_PX,
          justifyContent: "center",
          minHeight: APP_SIDEBAR_NAV_ITEM_MIN_HEIGHT_PX,
          paddingBlock: APP_SIDEBAR_NAV_ITEM_PADDING_BLOCK_PX,
          paddingInline: 0,
        },
        section: {
          marginInline: 0,
          marginInlineEnd: 0,
        },
      }}
      type="button"
      variant="light"
    />
  );
}

/** 渲染固定身份区、可滚动功能区和底部设置入口。 */
export function AppSidebar({
  activePath,
  applicationName,
  mode = "compact",
  version,
  onNavigate,
}: AppSidebarProps): ReactElement {
  const { t } = useTranslation();
  const dashboard: NavigationItem = {
    icon: IconLayoutDashboard,
    id: "dashboard",
    label: t("navigation.dashboard"),
    path: "/dashboard",
  };
  const monitor: NavigationItem = {
    icon: IconDeviceDesktopAnalytics,
    id: "monitor",
    label: t("navigation.monitor"),
    path: "/monitor",
  };
  const settings: NavigationItem = {
    icon: IconSettings,
    id: "settings",
    label: t("navigation.settings"),
    path: "/settings",
  };

  return (
    <Box
      aria-label={t("sidebar.application_navigation")}
      component="nav"
      data-mode={mode}
      data-testid="app-sidebar"
      style={{
        background: "var(--app-surface)",
        borderInlineEnd: "1px solid var(--app-border)",
        display: "flex",
        flexDirection: "column",
        height: "100dvh",
        insetBlockStart: 0,
        insetInlineStart: 0,
        padding: COMPACT_PADDING,
        position: "fixed",
        width: APP_SIDEBAR_WIDTHS.compact,
      }}
    >
      <Stack align="center" gap={SECTION_GAP} py={SECTION_GAP}>
        <Image
          alt={t("sidebar.logo_alt", { applicationName })}
          fit="contain"
          h={APP_SIDEBAR_LOGO_SIZES.compact}
          src={APP_SIDEBAR_LOGO_PATH}
          w={APP_SIDEBAR_LOGO_SIZES.compact}
        />
        <Text c="dimmed" size="10px">
          {formatDisplayVersion(version)}
        </Text>
      </Stack>

      <Divider my={SECTION_GAP} />
      <ScrollArea style={{ flex: 1, minHeight: 0 }} type="auto">
        <AppSidebarNavigationItem
          active={isDashboardLandingPath(activePath)}
          item={dashboard}
          onNavigate={onNavigate}
        />
        <AppSidebarNavigationItem
          active={activePath === "/monitor" || activePath.startsWith("/monitor/")}
          item={monitor}
          onNavigate={onNavigate}
        />
      </ScrollArea>
      <Divider my={SECTION_GAP} />
      <AppSidebarNavigationItem
        active={activePath === "/settings"}
        item={settings}
        onNavigate={onNavigate}
      />
    </Box>
  );
}
