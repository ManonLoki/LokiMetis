import { atom } from "jotai";

import type { AgentClientKind, UsageViewKind } from "../api/usage-types";

// `atom(初始值)` 创建一个 Jotai 原子——最小的一份全局状态。
// DashboardShell.tsx 用 `useAtom(agentClientAtom)` 读取并修改它
// （顶部导航栏的 Codex/Claude Code 切换控件），其余任何组件只需要
// `useAtomValue(agentClientAtom)` 只读订阅它的当前值——所有依赖当前
// 客户端的 useQuery 都会把它放进 queryKey，客户端一变，相关查询的
// queryKey 也跟着变，TanStack Query 就会自动重新请求对应客户端的数据。
/** 当前查看客户端是所有 Query、扫描和数据根操作的显式作用域。 */
export const agentClientAtom = atom<AgentClientKind>("codex");

/** 当前概览或调用的只读视图；只在当前桌面进程中保留。 */
export const usageViewAtom = atom<UsageViewKind>("codex");

/** 返回不随 locale 变化的客户端产品名称。 */
export function agentClientLabel(client: AgentClientKind): string {
  const labels: Record<AgentClientKind, string> = {
    claudeCode: "Claude Code",
    codex: "Codex",
    grokBuildCli: "Grok",
  };
  return labels[client];
}
