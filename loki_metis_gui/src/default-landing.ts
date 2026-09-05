/** 冷启动根路径、空路径和宿主 index.html 一律落到看板。 */
export function resolveDefaultLandingPath(pathname: string): string {
  if (pathname === "/" || pathname === "" || pathname === "/index.html") {
    return "/dashboard";
  }
  return pathname;
}

/** 判断当前路径是否应高亮侧栏看板，含尚未跳转的冷启动入口。 */
export function isDashboardLandingPath(pathname: string): boolean {
  return (
    resolveDefaultLandingPath(pathname) === "/dashboard" || pathname.startsWith("/dashboard/")
  );
}

/** 桌宠悬浮窗使用独立根，不挂主壳。 */
export function isPetOverlayPath(pathname: string): boolean {
  return pathname === "/pet" || pathname.startsWith("/pet/");
}
