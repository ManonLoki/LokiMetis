import { atom } from "jotai";

import type { InterfaceLanguage } from "../lib/api";

/** 当前进程中的界面语言镜像，用于跨路由同步控件。 */
export const interfaceLanguageAtom = atom<InterfaceLanguage>("en-US");
