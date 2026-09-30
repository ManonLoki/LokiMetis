import type { InterfaceLanguage } from "./api";

/** 本机保存界面语言时使用的唯一设备偏好键。 */
export const INTERFACE_LANGUAGE_STORAGE_KEY = "loki-metis.interface-language";

/** 把 BCP-47 语言标签归一化为应用支持的固定资源集合。 */
export function normalizeInterfaceLanguage(value: unknown): InterfaceLanguage {
  if (typeof value !== "string") return "en-US";
  const normalized = value.trim().replaceAll("_", "-").toLowerCase();
  if (normalized === "zh" || normalized.startsWith("zh-")) return "zh-CN";
  if (normalized === "en" || normalized.startsWith("en-")) return "en-US";
  return "en-US";
}

/** 安全读取已保存语言；非法值视为未保存而非英文选择。 */
export function readSavedInterfaceLanguage(): InterfaceLanguage | undefined {
  try {
    const value = window.localStorage.getItem(INTERFACE_LANGUAGE_STORAGE_KEY);
    return value === "zh-CN" || value === "en-US" ? value : undefined;
  } catch {
    return undefined;
  }
}

/** 保存宿主已确认的界面语言；存储不可用不阻断本次运行内切换。 */
export function persistInterfaceLanguage(language: InterfaceLanguage): void {
  try {
    window.localStorage.setItem(INTERFACE_LANGUAGE_STORAGE_KEY, language);
  } catch {
    // 本次运行内的 i18next 与 Jotai 状态仍保持一致。
  }
}
