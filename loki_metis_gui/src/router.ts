import { getCurrentWebviewWindow } from "@tauri-apps/api/webviewWindow";
import { createBrowserHistory, createRouter } from "@tanstack/react-router";

import { isPetOverlayPath, resolveDefaultLandingPath } from "./default-landing";
import { routeTree } from "./routeTree.gen";

/** 桌宠窗口生产入口是 index.html，按原生标签改写到 /pet。 */
function syncPetOverlayLocation(): void {
  try {
    if (getCurrentWebviewWindow().label !== "pet") {
      return;
    }
    if (!isPetOverlayPath(window.location.pathname)) {
      window.history.replaceState(null, "", "/pet");
    }
  } catch {
    return;
  }
}

syncPetOverlayLocation();
if (typeof document !== "undefined" && isPetOverlayPath(window.location.pathname)) {
  document.documentElement.classList.add("pet-window");
}
const history = createBrowserHistory();
const landingPath = isPetOverlayPath(history.location.pathname)
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
