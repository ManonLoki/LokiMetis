/**
 * 保存换皮页跨路由挂载仍需保留的纯会话交互状态，不镜像原生资源或查询结果。
 */
import { atom } from "jotai";

/** 描述换皮页当前进程内的筛选和宿主选择。 */
export interface SkinPageSession {
  search: string;
  userOnly: boolean;
  selectedHost: "codex" | "workBuddy" | null;
  restoreDismissedHosts: Partial<Record<"codex" | "workBuddy", boolean>>;
}

/** 全新应用 store 中换皮页使用的确定默认值。 */
export const initialSkinPageSession: SkinPageSession = {
  search: "",
  userOnly: false,
  selectedHost: null,
  restoreDismissedHosts: {},
};

/** 仅在当前应用进程中保留换皮页工作上下文。 */
export const skinPageSessionAtom = atom<SkinPageSession>(initialSkinPageSession);
