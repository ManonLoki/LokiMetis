import {
  ActionIcon,
  Box,
  Divider,
  Image,
  NavLink,
  ScrollArea,
  Stack,
  Text,
  Tooltip,
} from "@mantine/core";
import {
  IconChevronLeft,
  IconChevronRight,
  IconHeart,
  IconSettings,
  type TablerIcon,
} from "@tabler/icons-react";
import { type ReactElement } from "react";
import { useTranslation } from "react-i18next";

import {
  buildSupportNavigationItems,
  type SupportPageSelection,
  type SupportNavigationItem,
} from "./supportNavigation";
import { formatDisplayVersion } from "./displayVersion";

/** GUI 初始化可选择持续显示名称的精简模式或可收起的详细模式。 */
export type AppSidebarMode = "compact" | "detailed";

/** 详细模式首次启动默认展开，之后恢复设备级折叠偏好。 */
export const DEFAULT_DETAILED_SIDEBAR_COLLAPSED = false;

/** 详细模式折叠状态使用独立设备偏好键，不与页面会话状态混用。 */
export const APP_SIDEBAR_COLLAPSED_STORAGE_KEY =
  "app.sidebar.detailed.collapsed";

/** 侧栏身份只能引用 GUI bundle 中批准的应用 Logo。 */
export const APP_SIDEBAR_LOGO_PATH = "/app-identity/logo.png";

/** 两种模式及详细模式两种状态的固定宽度。 */
export const APP_SIDEBAR_WIDTHS = {
  compact: 80,
  detailedCollapsed: 76,
  detailedExpanded: 248,
} as const;

/** 精简和详细模式各状态使用的 Logo 尺寸。 */
export const APP_SIDEBAR_LOGO_SIZES = {
  compact: 36,
  detailedCollapsed: 44,
  detailedExpanded: 72,
} as const;

/** 精简栏内容使用明确像素，避免 Mantine xs padding 再压缩可用宽度。 */
export const APP_SIDEBAR_COMPACT_PADDING_PX = 6;

/** 精简栏身份区、分隔线与菜单区之间保持固定间距。 */
export const APP_SIDEBAR_COMPACT_SECTION_GAP_PX = 8;

/** 两种侧栏模式统一使用的菜单图标尺寸。 */
export const APP_SIDEBAR_NAV_ICON_SIZE_PX = 22;

/** 侧栏图标统一使用的 Tabler 描边宽度。 */
export const APP_SIDEBAR_ICON_STROKE_WIDTH = 1.75;

/** 详细展开态菜单项保持稳定点击高度。 */
export const APP_SIDEBAR_DETAILED_NAV_ITEM_MIN_HEIGHT_PX = 44;

/** 详细身份区折叠按钮使用的 Tabler 图标尺寸。 */
export const APP_SIDEBAR_COLLAPSE_ICON_SIZE_PX = 18;

/** 收起态 Tooltip 立即揭示被隐藏的菜单名称。 */
export const APP_SIDEBAR_TOOLTIP_OPEN_DELAY_MS = 0;

/** 功能区与固定支持区中的菜单项保持一致间距。 */
export const APP_SIDEBAR_MENU_STACK_GAP_PX = 4;

/** 精简菜单名称使用固定字号并允许最多两行自然换行。 */
export const APP_SIDEBAR_LABEL_FONT_SIZE_PX = 11;

/** 精简菜单名称允许最多两行，并使用固定行高。 */
export const APP_SIDEBAR_LABEL_LINE_HEIGHT = 1.25;

/** 精简菜单项保持稳定点击高度。 */
export const APP_SIDEBAR_COMPACT_NAV_ITEM_MIN_HEIGHT_PX = 56;

/** 精简菜单项只保留少量垂直内边距。 */
export const APP_SIDEBAR_COMPACT_NAV_ITEM_PADDING_BLOCK_PX = 4;

/** 精简菜单图标与名称之间保持固定间距。 */
export const APP_SIDEBAR_COMPACT_NAV_ITEM_GAP_PX = 4;

/** 产品功能菜单项由当前下游按显示顺序注入。 */
export interface FeatureNavigationItem {
  id: string;
  label: string;
  to: string;
  icon: TablerIcon;
}

/** 固定左侧菜单所需的当前应用事实和纯交互回调。 */
export interface AppSidebarTemplateProps {
  applicationName: string;
  logoSrc: string;
  version: string;
  featureItems: FeatureNavigationItem[];
  mode: AppSidebarMode;
  supportPages: SupportPageSelection;
  activePath: string;
  onNavigate: (path: string) => void;
  detailedCollapsed?: boolean;
  onCollapsedChange?: (collapsed: boolean) => void;
}

/** 固定支持菜单只使用统一的 Tabler 图标组件。 */
const FIXED_NAVIGATION_ICONS: Record<
  SupportNavigationItem["id"],
  TablerIcon
> = {
  sponsor: IconHeart,
  settings: IconSettings,
};

/** 读取详细模式折叠偏好；缺失、不可用或非法值都回到默认展开。 */
export function readDetailedSidebarCollapsed(): boolean {
  if (typeof window === "undefined") {
    return DEFAULT_DETAILED_SIDEBAR_COLLAPSED;
  }
  try {
    return (
      window.localStorage.getItem(APP_SIDEBAR_COLLAPSED_STORAGE_KEY) === "true"
    );
  } catch {
    return DEFAULT_DETAILED_SIDEBAR_COLLAPSED;
  }
}

/** 保存详细模式折叠偏好；存储不可用时仍保留 AppShell 本次运行内状态。 */
export function persistDetailedSidebarCollapsed(collapsed: boolean): void {
  if (typeof window === "undefined") {
    return;
  }
  try {
    window.localStorage.setItem(
      APP_SIDEBAR_COLLAPSED_STORAGE_KEY,
      String(collapsed),
    );
  } catch {
    // 设备级存储不可用时，当前 React 状态仍然有效。
  }
}

/** 详细 AppShell 与 fixed 侧栏共用同一宽度选择函数。 */
export function detailedSidebarNavbarWidth(collapsed: boolean): number {
  return collapsed
    ? APP_SIDEBAR_WIDTHS.detailedCollapsed
    : APP_SIDEBAR_WIDTHS.detailedExpanded;
}

/** 根据所选模式渲染精简竖排菜单或详细横排/折叠菜单。 */
function SidebarNavigationItem({
  active,
  collapsed,
  icon,
  id,
  label,
  mode,
  onClick,
}: {
  active: boolean;
  collapsed: boolean;
  icon: TablerIcon;
  id: string;
  label: string;
  mode: AppSidebarMode;
  onClick: () => void;
}): ReactElement {
  const IconComponent = icon;
  const compact = mode === "compact";
  const iconOnly = mode === "detailed" && collapsed;
  const navigation = (
    <NavLink
      active={active}
      aria-label={label}
      component="button"
      data-label-alignment={compact ? "full-width-center" : undefined}
      data-navigation-layout={
        compact ? "icon-above-label" : iconOnly ? "icon-only" : "icon-with-label"
      }
      label={
        iconOnly ? undefined : compact ? (
          <Text
            data-testid={`navigation-label-${id}`}
            lineClamp={2}
            style={{
              display: "block",
              fontSize: APP_SIDEBAR_LABEL_FONT_SIZE_PX,
              lineHeight: APP_SIDEBAR_LABEL_LINE_HEIGHT,
              marginInline: "auto",
              maxInlineSize: "100%",
              overflowWrap: "anywhere",
              textAlign: "center",
              width: "100%",
            }}
          >
            {label}
          </Text>
        ) : (
          <Text data-testid={`navigation-label-${id}`} lineClamp={2}>
            {label}
          </Text>
        )
      }
      leftSection={
        <IconComponent
          aria-hidden="true"
          data-testid={`navigation-icon-${id}`}
          size={APP_SIDEBAR_NAV_ICON_SIZE_PX}
          stroke={APP_SIDEBAR_ICON_STROKE_WIDTH}
        />
      }
      onClick={onClick}
      px={compact || iconOnly ? 0 : "sm"}
      styles={{
        body: {
          flex: compact || iconOnly ? "0 0 auto" : "1 1 auto",
          overflow: compact ? "visible" : undefined,
          textAlign: compact ? "center" : undefined,
          width: compact ? "100%" : undefined,
        },
        label: {
          display: compact ? "block" : undefined,
          marginInline: compact ? "auto" : undefined,
          textAlign: compact ? "center" : undefined,
          whiteSpace: compact ? "normal" : undefined,
          width: compact ? "100%" : undefined,
        },
        root: {
          alignItems: "center",
          flexDirection: compact ? "column" : "row",
          gap: compact ? APP_SIDEBAR_COMPACT_NAV_ITEM_GAP_PX : undefined,
          justifyContent: compact || iconOnly ? "center" : "flex-start",
          minHeight: compact
            ? APP_SIDEBAR_COMPACT_NAV_ITEM_MIN_HEIGHT_PX
            : APP_SIDEBAR_DETAILED_NAV_ITEM_MIN_HEIGHT_PX,
          paddingBlock: compact
            ? APP_SIDEBAR_COMPACT_NAV_ITEM_PADDING_BLOCK_PX
            : undefined,
          paddingInline: compact || iconOnly ? 0 : undefined,
        },
        section: {
          marginInline: compact || iconOnly ? 0 : undefined,
          marginInlineEnd: compact || iconOnly ? 0 : undefined,
        },
      }}
      type="button"
      variant="light"
    />
  );
  return iconOnly ? (
    <Tooltip
      label={label}
      openDelay={APP_SIDEBAR_TOOLTIP_OPEN_DELAY_MS}
      position="right"
      withArrow
    >
      {navigation}
    </Tooltip>
  ) : (
    navigation
  );
}

/** 渲染所选侧栏模式，功能从上向下增长且已选支持页面固定贴底。 */
export function AppSidebarTemplate({
  applicationName,
  logoSrc,
  version,
  featureItems,
  mode,
  supportPages,
  activePath,
  onNavigate,
  detailedCollapsed,
  onCollapsedChange,
}: AppSidebarTemplateProps): ReactElement {
  const { t } = useTranslation("brandSupport");
  const displayVersion = formatDisplayVersion(version);
  if (
    mode === "detailed" &&
    (typeof detailedCollapsed !== "boolean" || !onCollapsedChange)
  ) {
    throw new Error(
      "detailed sidebar must be controlled by AppShell width state",
    );
  }
  const collapsed = mode === "detailed" && detailedCollapsed === true;
  const sizeKey =
    mode === "compact"
      ? "compact"
      : collapsed
        ? "detailedCollapsed"
        : "detailedExpanded";
  const width = APP_SIDEBAR_WIDTHS[sizeKey];
  const logoSize = APP_SIDEBAR_LOGO_SIZES[sizeKey];
  const supportNavigationItems = buildSupportNavigationItems(supportPages);

  if (logoSrc !== APP_SIDEBAR_LOGO_PATH) {
    throw new Error(
      `application logo must use ${APP_SIDEBAR_LOGO_PATH}`,
    );
  }

  return (
    <Box
      aria-label={t("sidebar.application_navigation")}
      component="nav"
      data-collapsed={collapsed}
      data-layout={mode === "compact" ? "compact" : "detailed"}
      data-mode={mode}
      data-testid="app-sidebar"
      style={{
        background: "var(--app-surface)",
        borderInlineEnd: "1px solid var(--app-border)",
        height: "100dvh",
        insetBlock: 0,
        insetInlineStart: 0,
        position: "fixed",
        transition: "width 160ms ease",
        width,
        zIndex: 100,
      }}
    >
      <Stack
        data-testid="app-sidebar-content"
        gap={mode === "compact" ? APP_SIDEBAR_COMPACT_SECTION_GAP_PX : "sm"}
        h="100%"
        p={mode === "compact" ? APP_SIDEBAR_COMPACT_PADDING_PX : 0}
      >
        <Stack
          align="center"
          data-icon-alignment="center"
          data-testid="app-sidebar-identity"
          gap="xs"
          p={mode === "detailed" ? "xs" : 0}
          style={{ alignItems: "center", width: "100%" }}
        >
          <Image
            alt={t("sidebar.logo_alt", { applicationName })}
            data-testid="app-sidebar-logo"
            fit="contain"
            h={logoSize}
            src={logoSrc}
            w={logoSize}
          />
          <Text
            aria-label={`${applicationName} ${t("sidebar.version", { version: displayVersion })}`}
            c="dimmed"
            data-testid="app-sidebar-version"
            fw={600}
            lineClamp={1}
            size="xs"
            ta="center"
          >
            {displayVersion}
          </Text>
          {mode === "detailed" ? (
            <ActionIcon
              aria-label={
                collapsed ? t("sidebar.expand") : t("sidebar.collapse")
              }
              data-testid="app-sidebar-collapse-toggle"
              onClick={() => {
                const nextCollapsed = !collapsed;
                onCollapsedChange?.(nextCollapsed);
              }}
              type="button"
              variant="subtle"
            >
              {collapsed ? (
                <IconChevronRight
                  aria-hidden="true"
                  size={APP_SIDEBAR_COLLAPSE_ICON_SIZE_PX}
                  stroke={APP_SIDEBAR_ICON_STROKE_WIDTH}
                />
              ) : (
                <IconChevronLeft
                  aria-hidden="true"
                  size={APP_SIDEBAR_COLLAPSE_ICON_SIZE_PX}
                  stroke={APP_SIDEBAR_ICON_STROKE_WIDTH}
                />
              )}
            </ActionIcon>
          ) : null}
        </Stack>

        <Divider />

        <ScrollArea
          data-testid="feature-navigation"
          style={{ flex: 1, minHeight: 0 }}
          type="auto"
        >
          <Stack gap={APP_SIDEBAR_MENU_STACK_GAP_PX}>
            {featureItems.map((item) => (
              <SidebarNavigationItem
                active={activePath === item.to}
                collapsed={collapsed}
                icon={item.icon}
                id={item.id}
                key={item.id}
                label={item.label}
                mode={mode}
                onClick={() => onNavigate(item.to)}
              />
            ))}
          </Stack>
        </ScrollArea>

        <Divider />

        <Stack
          data-testid="fixed-bottom-navigation"
          gap={APP_SIDEBAR_MENU_STACK_GAP_PX}
        >
          {supportNavigationItems.map((item) => {
            const label = t(item.labelKey);
            return (
              <SidebarNavigationItem
                active={activePath === item.to}
                collapsed={collapsed}
                icon={FIXED_NAVIGATION_ICONS[item.id]}
                id={item.id}
                key={item.id}
                label={label}
                mode={mode}
                onClick={() => onNavigate(item.to)}
              />
            );
          })}
        </Stack>
      </Stack>
    </Box>
  );
}
