import { createRouter } from "@tanstack/react-router";

import { routeTree } from "./routeTree.gen";

/** 应用唯一文件路由实例。 */
export const router = createRouter({
  defaultPreload: "intent",
  defaultPreloadStaleTime: 0,
  routeTree,
});

declare module "@tanstack/react-router" {
  /** 把生成路由树注册到 TanStack Router 类型系统。 */
  interface Register {
    router: typeof router;
  }
}
