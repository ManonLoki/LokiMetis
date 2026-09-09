import {
  Box,
  MantineProvider,
  createTheme,
  localStorageColorSchemeManager,
  useComputedColorScheme,
  type CSSVariablesResolver,
} from "@mantine/core";
import type { ReactElement, ReactNode } from "react";

/** 设置页支持的设备级主题偏好。 */
export type AppColorScheme = "light" | "dark" | "auto";

/** 主题偏好使用的唯一设备存储键。 */
export const APP_COLOR_SCHEME_STORAGE_KEY = "loki-metis.color-scheme";

/** 中性初始化使用的唯一 Mantine 主题。 */
export const APP_THEME = createTheme({
  defaultRadius: "md",
  fontFamily:
    "Inter, ui-sans-serif, system-ui, -apple-system, BlinkMacSystemFont, Segoe UI, sans-serif",
  primaryColor: "indigo",
  headings: {
    fontFamily:
      "Inter, ui-sans-serif, system-ui, -apple-system, BlinkMacSystemFont, Segoe UI, sans-serif",
    fontWeight: "650",
  },
});

/** 为亮暗主题提供稳定的应用级语义颜色。 */
export const APP_THEME_VARIABLES: CSSVariablesResolver = (theme) => ({
  variables: {},
  light: {
    "--app-accent": theme.colors.indigo[7],
    "--app-background": "#f4f5fa",
    "--app-border": "#dfe2eb",
    "--app-glow": "rgba(83, 93, 178, 0.16)",
    "--app-surface": "rgba(255, 255, 255, 0.88)",
    "--app-text": theme.colors.dark[9],
    "--app-text-muted": theme.colors.gray[7],
    "--app-muted": "#eef0f6",
    "--app-card-border": "#dfe2eb",
    "--app-card-shadow": "0 16px 42px -38px rgba(15, 23, 42, 0.72)",
    "--app-control-hover-border": theme.colors.indigo[4],
    "--app-violet-text": theme.colors.indigo[7],
    "--app-violet-border": theme.colors.indigo[2],
    "--app-control-background": "#ffffff",
    "--app-endpoint-background": "#fafaff",
    "--app-endpoint-border": "#e1e3e9",
    "--app-image-hover-border": "#c8cbd5",
    "--app-selected-border": theme.colors.indigo[6],
    "--app-slot-background": "#ffffff",
    "--app-empty-icon-color": theme.colors.indigo[6],
    "--app-empty-icon-background": theme.colors.indigo[0],
    "--app-dashboard-toolbar-background": "rgba(255, 252, 247, 0.92)",
    "--app-dashboard-toolbar-border": "#eadfd0",
    "--app-dashboard-nav-text": "#655d54",
    "--app-dashboard-nav-hover-text": "#a52f25",
    "--app-dashboard-nav-hover-background": "#fff0e5",
    "--app-dashboard-nav-active-text": "#94291f",
    "--app-dashboard-nav-active-background": "#fbe2d6",
    "--app-dashboard-nav-focus-ring": "rgba(196, 59, 45, 0.24)",
    "--app-data-panel-background": "rgba(255, 255, 255, 0.82)",
    "--app-data-panel-border": "rgba(103, 81, 60, 0.15)",
    "--app-data-panel-shadow": "rgba(91, 63, 36, 0.07)",
    "--app-mini-metric-background": "#fcfaf7",
    "--app-mini-metric-border": "rgba(103, 81, 60, 0.1)",
    "--app-data-divider": "rgba(103, 81, 60, 0.12)",
    "--app-chart-text": "#655d54",
    "--app-chart-grid": "rgba(103, 81, 60, 0.16)",
    "--app-chart-hover-line": "rgba(75, 62, 49, 0.5)",
    "--app-chart-point-stroke": "#ffffff",
    "--app-chart-tooltip-background": "rgba(255, 255, 255, 0.97)",
    "--app-chart-tooltip-border": "rgba(103, 81, 60, 0.24)",
    "--app-chart-tooltip-text": "#332b24",
    "--app-chart-bar-track": "rgba(103, 81, 60, 0.09)",
    "--app-chart-bar": "#d97706",
    "--app-chart-bar-remainder": "#78716c",
    "--app-chart-focus-stroke": "rgba(217, 119, 6, 0.72)",
  },
  dark: {
    "--app-accent": theme.colors.indigo[3],
    "--app-background": "#101116",
    "--app-border": "#2e3039",
    "--app-glow": "rgba(116, 124, 212, 0.14)",
    "--app-surface": "rgba(27, 29, 36, 0.9)",
    "--app-text": theme.colors.gray[0],
    "--app-text-muted": theme.colors.dark[1],
    "--app-muted": "#1c1e26",
    "--app-card-border": "#2e3039",
    "--app-card-shadow": "0 16px 42px -38px rgba(0, 0, 0, 0.72)",
    "--app-control-hover-border": theme.colors.indigo[5],
    "--app-violet-text": theme.colors.indigo[3],
    "--app-violet-border": theme.colors.indigo[7],
    "--app-control-background": "#1b1d24",
    "--app-endpoint-background": "#26282f",
    "--app-endpoint-border": "#3b3e48",
    "--app-image-hover-border": "#515563",
    "--app-selected-border": theme.colors.indigo[5],
    "--app-slot-background": "#16181f",
    "--app-empty-icon-color": theme.colors.indigo[3],
    "--app-empty-icon-background": theme.colors.dark[6],
    "--app-dashboard-toolbar-background": "rgba(24, 25, 27, 0.94)",
    "--app-dashboard-toolbar-border": "#38332e",
    "--app-dashboard-nav-text": "#bdb7af",
    "--app-dashboard-nav-hover-text": "#ff9b6f",
    "--app-dashboard-nav-hover-background": "#34231e",
    "--app-dashboard-nav-active-text": "#ffb18e",
    "--app-dashboard-nav-active-background": "#3c241d",
    "--app-dashboard-nav-focus-ring": "rgba(255, 155, 111, 0.35)",
    "--app-data-panel-background": "rgba(31, 32, 35, 0.9)",
    "--app-data-panel-border": "rgba(255, 255, 255, 0.1)",
    "--app-data-panel-shadow": "rgba(0, 0, 0, 0.12)",
    "--app-mini-metric-background": "#28292c",
    "--app-mini-metric-border": "rgba(255, 255, 255, 0.08)",
    "--app-data-divider": "rgba(255, 255, 255, 0.08)",
    "--app-chart-text": "#c8c2ba",
    "--app-chart-grid": "rgba(255, 255, 255, 0.12)",
    "--app-chart-hover-line": "rgba(255, 255, 255, 0.52)",
    "--app-chart-point-stroke": "#1f2023",
    "--app-chart-tooltip-background": "rgba(28, 29, 32, 0.97)",
    "--app-chart-tooltip-border": "rgba(255, 255, 255, 0.18)",
    "--app-chart-tooltip-text": "#f1ede8",
    "--app-chart-bar-track": "rgba(255, 255, 255, 0.08)",
    "--app-chart-bar": "#fb923c",
    "--app-chart-bar-remainder": "#a8a29e",
    "--app-chart-focus-stroke": "rgba(251, 146, 60, 0.82)",
  },
});

const colorSchemeManager = localStorageColorSchemeManager({
  key: APP_COLOR_SCHEME_STORAGE_KEY,
});

/** 把当前解析后的主题应用到整棵页面。 */
function ThemeSurface({ children }: { children: ReactNode }): ReactElement {
  const colorScheme = useComputedColorScheme("light");
  const petOverlay =
    typeof document !== "undefined" &&
    document.documentElement.classList.contains("pet-window");
  const petSettings =
    typeof document !== "undefined" &&
    document.documentElement.classList.contains("pet-settings-window");
  const petAuxiliaryWindow = petOverlay || petSettings;
  return (
    <Box
      data-color-scheme={colorScheme}
      data-testid="app-theme-surface"
      mih={petAuxiliaryWindow ? "100%" : "100dvh"}
      style={{
        background: petOverlay
          ? "transparent"
          : petSettings
            ? "#11151d"
            : "var(--app-background)",
        color: "var(--app-text)",
        height: petAuxiliaryWindow ? "100%" : undefined,
      }}
    >
      {children}
    </Box>
  );
}

/** 挂载应用唯一 Mantine Provider，并允许测试宿主显式关闭动画与 Portal。 */
export function AppThemeProvider({
  children,
  environment = "default",
}: {
  children: ReactNode;
  environment?: "default" | "test";
}): ReactElement {
  return (
    <MantineProvider
      colorSchemeManager={colorSchemeManager}
      cssVariablesResolver={APP_THEME_VARIABLES}
      defaultColorScheme="auto"
      env={environment}
      theme={APP_THEME}
    >
      <ThemeSurface>{children}</ThemeSurface>
    </MantineProvider>
  );
}
