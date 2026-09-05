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
    "--app-violet-text": theme.colors.indigo[7],
    "--app-control-background": "#ffffff",
    "--app-selected-border": theme.colors.indigo[6],
    "--app-slot-background": "#ffffff",
    "--app-empty-icon-color": theme.colors.indigo[6],
    "--app-empty-icon-background": theme.colors.indigo[0],
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
    "--app-violet-text": theme.colors.indigo[3],
    "--app-control-background": "#1b1d24",
    "--app-selected-border": theme.colors.indigo[5],
    "--app-slot-background": "#16181f",
    "--app-empty-icon-color": theme.colors.indigo[3],
    "--app-empty-icon-background": theme.colors.dark[6],
  },
});

const colorSchemeManager = localStorageColorSchemeManager({
  key: APP_COLOR_SCHEME_STORAGE_KEY,
});

/** 把当前解析后的主题应用到整棵页面。 */
function ThemeSurface({ children }: { children: ReactNode }): ReactElement {
  const colorScheme = useComputedColorScheme("light");
  const petOverlay =
    typeof document !== "undefined" && document.documentElement.classList.contains("pet-window");
  return (
    <Box
      data-color-scheme={colorScheme}
      data-testid="app-theme-surface"
      mih={petOverlay ? "100%" : "100dvh"}
      style={{
        background: petOverlay ? "transparent" : "var(--app-background)",
        color: "var(--app-text)",
      }}
    >
      {children}
    </Box>
  );
}

/** 挂载应用唯一 Mantine Provider，并默认跟随系统主题。 */
export function AppThemeProvider({ children }: { children: ReactNode }): ReactElement {
  return (
    <MantineProvider
      colorSchemeManager={colorSchemeManager}
      cssVariablesResolver={APP_THEME_VARIABLES}
      defaultColorScheme="auto"
      theme={APP_THEME}
    >
      <ThemeSurface>{children}</ThemeSurface>
    </MantineProvider>
  );
}
