/** 侧栏、排行榜和设置卡共享 Provider 配置快照。 */
export const COLLECT_STATUS_QUERY_KEY = ["collect-provider-status"] as const;

/** 排行榜全部 Provider/窗口快照共享的缓存前缀。 */
export const PROVIDER_LEADERBOARD_QUERY_SCOPE = "provider-leaderboard";

/** 换皮资源库的共享查询键。 */
export const SKIN_CATALOG_QUERY_KEY = ["skin-catalog"] as const;
/** 当前皮肤注入状态的共享查询键。 */
export const skinStatusQueryKey = (host: string) => ["skin-status", host] as const;
/** 指定换皮宿主 GUI 运行状态的共享查询键。 */
export const skinRuntimeQueryKey = (host: string) => ["skin-runtime", host] as const;
/** 指定换皮宿主实例列表的共享查询键。 */
export const skinInstancesQueryKey = (host: string) => ["skin-instances", host] as const;
