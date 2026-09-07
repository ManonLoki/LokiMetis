import type { SkinReference } from "../api/skins";

const REMEMBERED_SKIN_KEY = "loki-metis.remembered-skin.v1";

/** 判断未知值是否为可安全恢复的精确皮肤引用。 */
function isSkinReference(value: unknown): value is SkinReference {
  if (typeof value !== "object" || value === null) return false;
  const candidate = value as Record<string, unknown>;
  return (
    (candidate.source === "builtin" || candidate.source === "user") &&
    typeof candidate.id === "string" &&
    /^[a-z0-9][a-z0-9_-]{0,63}$/.test(candidate.id)
  );
}

/** 读取仅保存在本机 WebView 配置域中的上次成功皮肤。 */
export function readRememberedSkin(): SkinReference | null {
  try {
    const raw = window.localStorage.getItem(REMEMBERED_SKIN_KEY);
    if (raw === null) return null;
    const parsed: unknown = JSON.parse(raw);
    return isSkinReference(parsed) ? parsed : null;
  } catch {
    return null;
  }
}

/** 仅在原生层确认应用成功后记住精确皮肤身份。 */
export function rememberSkin(skin: SkinReference): void {
  window.localStorage.setItem(REMEMBERED_SKIN_KEY, JSON.stringify(skin));
}

/** 在用户停止皮肤后清除恢复提示来源。 */
export function clearRememberedSkin(): void {
  window.localStorage.removeItem(REMEMBERED_SKIN_KEY);
}
