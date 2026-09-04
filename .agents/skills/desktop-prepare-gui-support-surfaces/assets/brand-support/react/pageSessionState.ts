import { atom } from "jotai";

export type PageQueryStatus = "loading" | "success" | "error";

/** Sort criteria, when a page needs them, live inside `TQuery` — there is no separate sort field. */
export interface PageSessionState<TTab extends string, TQuery> {
  activeTab: TTab;
  query: TQuery;
  page: number;
  pageSize: number;
}

export interface PageSelection<TTab extends string, TQuery> {
  activeTab: TTab;
  query: TQuery;
}

export interface PageResultSnapshot {
  status: PageQueryStatus;
  itemCount: number;
}

function requirePositiveInteger(value: number, field: string): number {
  if (!Number.isInteger(value) || value < 1) {
    throw new Error(`${field} must be a positive integer`);
  }
  return value;
}

/**
 * 创建由应用根 Jotai store 持有的页面会话状态。
 *
 * 调用方必须在页面模块顶层创建一次，不能在 route 组件渲染期间创建；该状态只在
 * 当前应用进程内跨路由保留，不得接入任何浏览器或 Tauri 持久化适配器。
 */
export function createPageSessionState<TTab extends string, TQuery>(
  initialState: PageSessionState<TTab, TQuery>,
) {
  const normalizedInitialState: PageSessionState<TTab, TQuery> = {
    ...initialState,
    page: requirePositiveInteger(initialState.page, "page"),
    pageSize: requirePositiveInteger(initialState.pageSize, "pageSize"),
  };
  const stateAtom = atom(normalizedInitialState);

  const setSelectionAtom = atom(
    null,
    (_get, set, selection: PageSelection<TTab, TQuery>) => {
      set(stateAtom, (current) => ({
        ...current,
        activeTab: selection.activeTab,
        query: selection.query,
        page: 1,
      }));
    },
  );

  const setPaginationAtom = atom(
    null,
    (_get, set, pagination: Pick<PageSessionState<TTab, TQuery>, "page" | "pageSize">) => {
      const page = requirePositiveInteger(pagination.page, "page");
      const pageSize = requirePositiveInteger(pagination.pageSize, "pageSize");
      set(stateAtom, (current) => ({
        ...current,
        page: pageSize === current.pageSize ? page : 1,
        pageSize,
      }));
    },
  );

  const reconcilePageAfterResultAtom = atom(
    null,
    (get, set, result: PageResultSnapshot): boolean => {
      if (!Number.isInteger(result.itemCount) || result.itemCount < 0) {
        throw new Error("itemCount must be a non-negative integer");
      }
      const current = get(stateAtom);
      if (result.status !== "success" || result.itemCount > 0 || current.page === 1) {
        return false;
      }
      set(stateAtom, { ...current, page: 1 });
      return true;
    },
  );

  return Object.freeze({
    stateAtom,
    setSelectionAtom,
    setPaginationAtom,
    reconcilePageAfterResultAtom,
  });
}
