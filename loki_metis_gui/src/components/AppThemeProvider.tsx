import {
  Alert,
  defaultVariantColorsResolver,
  parseThemeColor,
  type VariantColorsResolver,
  Box,
  Tabs,
  MantineProvider,
  createTheme,
  type MantineColorsTuple,
  localStorageColorSchemeManager,
  useComputedColorScheme,
  type CSSVariablesResolver,
} from "@mantine/core";
import type { ReactElement, ReactNode } from "react";

/** 设置页支持的设备级主题偏好。 */
export type AppColorScheme = "light" | "dark" | "auto";

/** 主题偏好使用的唯一设备存储键。 */
export const APP_COLOR_SCHEME_STORAGE_KEY = "loki-metis.color-scheme";

/** 品牌暖红色阶，亮色取 6、暗色取 5，是全应用唯一强调色。 */
const BRAND_COLORS: MantineColorsTuple = [
  "#fff2ee",
  "#fde1d8",
  "#f8c2b1",
  "#f2a08a",
  "#ea7b62",
  "#d95a44",
  "#c43b2d",
  "#a52f25",
  "#87261e",
  "#6b1e18",
];

/** 浅色变体统一为“淡底加细边”：提示框、徽标与浅色按钮在亮暗主题下都不再是整块深色填充。 */
const variantColorResolver: VariantColorsResolver = (input) => {
  const base = defaultVariantColorsResolver(input);
  if (input.variant !== "light") return base;
  const { value } = parseThemeColor({ color: input.color ?? "gray", theme: input.theme });
  return {
    ...base,
    background: `color-mix(in srgb, ${value} 9%, transparent)`,
    border: `1px solid color-mix(in srgb, ${value} 26%, transparent)`,
    hover: `color-mix(in srgb, ${value} 15%, transparent)`,
  };
};

/** 中性初始化使用的唯一 Mantine 主题。 */
export const APP_THEME = createTheme({
  defaultRadius: "md",
  radius: { xs: "0.25rem", sm: "0.5rem", md: "0.75rem", lg: "1.25rem", xl: "1.75rem" },
  fontFamily:
    "Geist Variable, ui-sans-serif, system-ui, -apple-system, BlinkMacSystemFont, Segoe UI, PingFang SC, Microsoft YaHei, sans-serif",
  colors: { brand: BRAND_COLORS },
  fontFamilyMonospace:
    "Geist Mono Variable, ui-monospace, SFMono-Regular, Menlo, Consolas, monospace",
  primaryColor: "brand",
  variantColorResolver,
  primaryShade: { light: 6, dark: 5 },
  defaultGradient: { from: "brand.6", to: "brand.4", deg: 135 },
  components: {
    Alert: Alert.extend({
      defaultProps: { color: "gray", radius: "lg", variant: "light" },
    }),
    Tabs: Tabs.extend({ defaultProps: { className: "app-tabs" } }),
  },
  headings: {
    fontFamily:
      "Geist Variable, ui-sans-serif, system-ui, -apple-system, BlinkMacSystemFont, Segoe UI, PingFang SC, Microsoft YaHei, sans-serif",
    fontWeight: "650",
    sizes: {
      h2: { fontSize: "1.5rem", lineHeight: "1.25" },
      h3: { fontSize: "1.2rem", lineHeight: "1.3" },
      h4: { fontSize: "1rem", lineHeight: "1.35" },
    },
  },
});

/** 为亮暗主题提供稳定的应用级语义颜色。 */
export const APP_THEME_VARIABLES: CSSVariablesResolver = (theme) => ({
  variables: {
    "--app-ease": "cubic-bezier(0.16, 1, 0.3, 1)",
  },
  light: {
    "--app-accent": BRAND_COLORS[7],
    "--app-background": "#f7f4ef",
    "--app-border": "#e6ddd1",
    "--app-surface": "rgba(255, 253, 249, 0.9)",
    "--app-text": theme.colors.dark[9],
    "--app-muted": "#f0ebe3",
    "--app-card-border": "#e6ddd1",
    "--app-card-shadow": "0 20px 40px -22px rgba(91, 63, 36, 0.16)",
    "--app-card-shadow-hover": "0 24px 48px -20px rgba(91, 63, 36, 0.24)",
    "--app-control-hover-border": BRAND_COLORS[4],
    "--app-brand-text": BRAND_COLORS[7],
    "--app-brand-border": BRAND_COLORS[2],
    "--app-control-background": "#ffffff",
    "--app-endpoint-background": "#fcfaf6",
    "--app-endpoint-border": "#e4dcd0",
    "--app-image-hover-border": "#cfc5b6",
    "--app-selected-border": BRAND_COLORS[6],
    "--app-slot-background": "#ffffff",
    "--app-empty-icon-color": BRAND_COLORS[6],
    "--app-empty-icon-background": BRAND_COLORS[0],
    "--app-toolbar-background": "rgba(255, 252, 247, 0.92)",
    "--app-toolbar-border": "#eadfd0",
    "--app-nav-text": "#655d54",
    "--app-nav-hover-text": "#2f2922",
    "--app-nav-hover-background": "#f0e9de",
    "--app-nav-active-text": "#94291f",
    "--app-nav-active-background": "#fbe2d6",
    "--app-nav-focus-ring": "rgba(196, 59, 45, 0.24)",
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
    "--app-panel-edge": "inset 0 1px 0 rgba(255, 255, 255, 0.7)",
    "--app-glass-edge": "rgba(255, 255, 255, 0.75)",
  },
  dark: {
    "--app-accent": BRAND_COLORS[3],
    "--app-background": "#131211",
    "--app-border": "#332f2a",
    "--app-surface": "rgba(30, 28, 26, 0.92)",
    "--app-text": theme.colors.gray[0],
    "--app-muted": "#1e1c1a",
    "--app-card-border": "#332f2a",
    "--app-card-shadow": "0 16px 42px -38px rgba(0, 0, 0, 0.72)",
    "--app-card-shadow-hover": "0 20px 48px -30px rgba(0, 0, 0, 0.85)",
    "--app-control-hover-border": BRAND_COLORS[5],
    "--app-brand-text": BRAND_COLORS[3],
    "--app-brand-border": BRAND_COLORS[7],
    "--app-control-background": "#1e1c1a",
    "--app-endpoint-background": "#282521",
    "--app-endpoint-border": "#3d3832",
    "--app-image-hover-border": "#5a544b",
    "--app-selected-border": BRAND_COLORS[5],
    "--app-slot-background": "#191715",
    "--app-empty-icon-color": BRAND_COLORS[3],
    "--app-empty-icon-background": theme.colors.dark[6],
    "--app-toolbar-background": "rgba(24, 25, 27, 0.94)",
    "--app-toolbar-border": "#38332e",
    "--app-nav-text": "#bdb7af",
    "--app-nav-hover-text": "#f1ede8",
    "--app-nav-hover-background": "#2a2622",
    "--app-nav-active-text": "#ffb18e",
    "--app-nav-active-background": "#3c241d",
    "--app-nav-focus-ring": "rgba(255, 155, 111, 0.35)",
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
    "--app-panel-edge": "inset 0 1px 0 rgba(255, 255, 255, 0.06)",
    "--app-glass-edge": "rgba(255, 255, 255, 0.07)",
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
