import i18n, { type i18n as I18nInstance } from "i18next";
import { initReactI18next } from "react-i18next";

import { dashboardEnUS } from "./dashboardEn";
import { dashboardZhCN } from "./dashboardZh";
import enUS from "./locales/en-US.json";
import zhCN from "./locales/zh-CN.json";

/** 应用唯一 i18next 实例，组件不得建立第二套翻译状态。 */
export const appI18n: I18nInstance = i18n.createInstance();

/** 注册完整双语资源并以英文作为缺失资源回退。 */
export async function initializeI18n(): Promise<void> {
  if (appI18n.isInitialized) return;
  await appI18n.use(initReactI18next).init({
    resources: {
      "en-US": { translation: { ...dashboardEnUS, ...enUS } },
      "zh-CN": { translation: { ...dashboardZhCN, ...zhCN } },
    },
    fallbackLng: "en-US",
    supportedLngs: ["zh-CN", "en-US"],
    nonExplicitSupportedLngs: false,
    interpolation: { escapeValue: false },
    returnNull: false,
  });
}
