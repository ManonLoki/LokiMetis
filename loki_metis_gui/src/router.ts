import { getCurrentWebviewWindow } from "@tauri-apps/api/webviewWindow";
import { createBrowserHistory, createRouter } from "@tanstack/react-router";

import {
  isPetOverlayPath,
  isPetSettingsPath,
  isPetWindowPath,
  resolveDefaultLandingPath,
} from "./default-landing";
import { routeTree } from "./routeTree.gen";

/** 桌宠原生窗口共用 index.html，按标签改写到各自独立路由。 */
function syncPetWindowLocation(): void {
  try {
    const label = getCurrentWebviewWindow().label;
    if (label === "pet" && !isPetOverlayPath(window.location.pathname)) {
      window.history.replaceState(null, "", "/pet");
    }
    if (label === "pet-settings" && !isPetSettingsPath(window.location.pathname)) {
      window.history.replaceState(null, "", "/pet-settings");
    }
  } catch {
    return;
  }
}

syncPetWindowLocation();
if (typeof document !== "undefined" && isPetOverlayPath(window.location.pathname)) {
  document.documentElement.classList.add("pet-window");
}
if (typeof document !== "undefined" && isPetSettingsPath(window.location.pathname)) {
  document.documentElement.classList.add("pet-settings-window");
}
const history = createBrowserHistory();
const landingPath = isPetWindowPath(history.location.pathname)
  ? history.location.pathname
  : resolveDefaultLandingPath(history.location.pathname);
if (landingPath !== history.location.pathname) {
  history.replace(landingPath);
}

/** 应用唯一文件路由实例；冷启动入口在首屏前改写为看板。 */
export const router = createRouter({
  defaultPreload: "intent",
  defaultPreloadStaleTime: 0,
  history,
  routeTree,
});

declare module "@tanstack/react-router" {
  /** 把生成路由树注册到 TanStack Router 类型系统。 */
  interface Register {
    router: typeof router;
  }
}
