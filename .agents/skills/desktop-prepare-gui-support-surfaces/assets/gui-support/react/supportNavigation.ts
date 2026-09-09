/** 固定设置菜单项的稳定结构。 */
export interface SupportNavigationItem {
  id: "settings";
  labelKey: "navigation.settings";
  to: "/settings";
}

/** 设置是 GUI 壳层唯一固定支持入口。 */
export const FIXED_SUPPORT_NAVIGATION_ITEMS = [
  {
    id: "settings",
    labelKey: "navigation.settings",
    to: "/settings",
  },
] as const satisfies readonly SupportNavigationItem[];

/** 支持页面路由，供文件路由与菜单回归复用。 */
export type SupportNavigationPath = SupportNavigationItem["to"];
