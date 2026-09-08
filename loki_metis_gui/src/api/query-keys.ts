/** 换皮资源库的共享查询键。 */
export const SKIN_CATALOG_QUERY_KEY = ["skin-catalog"] as const;
/** 当前皮肤注入状态的共享查询键。 */
export const skinStatusQueryKey = (host: string) => ["skin-status", host] as const;
/** 指定换皮宿主实例列表的共享查询键。 */
export const skinInstancesQueryKey = (host: string) => ["skin-instances", host] as const;
