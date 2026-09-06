import { atom } from "jotai";

import type {
  UsageViewKind,
  UsageCallFiltersDto,
  UsageCallSortDirection,
  UsageCallSortField,
} from "../api/usage";
import { usageViewAtom } from "./agent-client";

/** 提供不含路径和正文的调用筛选初始值。 */
export const emptyUsageFilters: UsageCallFiltersDto = {
  model: null,
  project: null,
  reasoningEffort: null,
  root: null,
  thread: null,
};

/** 保存调用页的固定排序。 */
export interface UsageCallSort {
  direction: UsageCallSortDirection;
  field: UsageCallSortField;
}

/** 保存单个只读视图的调用筛选、排序与分页游标状态。 */
interface ClientCallState {
  filters: UsageCallFiltersDto;
  sort: UsageCallSort;
}

const defaultSort: UsageCallSort = { direction: "desc", field: "occurredAt" };

/** 返回互不共享引用的调用页初始状态。 */
function initialClientState(): ClientCallState {
  return { filters: { ...emptyUsageFilters }, sort: { ...defaultSort } };
}

// 真正的状态是“按只读视图分开保存”的一份记录（全部与三个物理 Agent
// 各自独立的筛选/排序），存在这一个当前进程内的底层原子里。
const callStateByClientAtom = atom<Record<UsageViewKind, ClientCallState>>({
  all: initialClientState(),
  claudeCode: initialClientState(),
  codex: initialClientState(),
  grokBuildCli: initialClientState(),
  workbuddy: initialClientState(),
});

// Jotai 的“派生可写原子”：`atom(读函数, 写函数)` 这种两参数写法创建出
// 一个对外表现得像“单一客户端筛选值”的原子，但内部读写时都会先结合
// `usageViewAtom` 的当前值去 callStateByClientAtom 里定位到具体是哪一个
// 只读视图的状态。这样组件代码可以直接
// `useAtom(usageFiltersAtom)` 当作"当前客户端的筛选"来用，完全不用
// 关心底层其实是按客户端分桶存储的，也不需要每次手动传 client 参数——
// 切换视图后自动读写到正确的那一份，天然保证“筛选/排序切换视图不
// 串数据"这条产品规则。
/** 只读写当前客户端的调用筛选，客户端切换不会继承另一个客户端的 ID。 */
export const usageFiltersAtom = atom(
  (get) => {
    const selectedClient = get(usageViewAtom);
    const client = selectedClient;
    return get(callStateByClientAtom)[client].filters;
  },
  (get, set, filters: UsageCallFiltersDto) => {
    const selectedClient = get(usageViewAtom);
    const client = selectedClient;
    const current = get(callStateByClientAtom);
    set(callStateByClientAtom, {
      ...current,
      [client]: { ...current[client], filters },
    });
  },
);

/** 只读写当前客户端的排序，切回时恢复该客户端自己的选择。 */
export const usageSortAtom = atom(
  (get) => {
    const selectedClient = get(usageViewAtom);
    const client = selectedClient;
    return get(callStateByClientAtom)[client].sort;
  },
  (get, set, sort: UsageCallSort) => {
    const selectedClient = get(usageViewAtom);
    const client = selectedClient;
    const current = get(callStateByClientAtom);
    set(callStateByClientAtom, {
      ...current,
      [client]: { ...current[client], sort },
    });
  },
);
