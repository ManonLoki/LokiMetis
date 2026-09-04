import { queryOptions } from "@tanstack/react-query";

import { getAppMetadata, getAutostartEnabled, getSystemNotificationSetting } from "./api";

/** 应用元数据查询在整个壳层中复用同一缓存。 */
export const appMetadataQuery = queryOptions({
  queryKey: ["app-metadata"],
  queryFn: () => getAppMetadata(),
  staleTime: Number.POSITIVE_INFINITY,
  retry: false,
});

/** 系统通知状态始终以宿主读取结果为准。 */
export const systemNotificationQuery = queryOptions({
  queryKey: ["host-capability", "system-notification"],
  queryFn: () => getSystemNotificationSetting(),
  retry: false,
});

/** 开机自启状态始终以操作系统登录项为准。 */
export const autostartQuery = queryOptions({
  queryKey: ["host-capability", "autostart"],
  queryFn: () => getAutostartEnabled(),
  retry: false,
});
