import { appI18n } from "./i18n";

/** 允许直接面向用户翻译的结构化 Rust 错误；其他错误仍折叠为安全兜底文案。 */
const VISIBLE_RUST_ERROR_CODES = new Set([
  "error.hooks.cleanupUnsupported",
  "error.hooks.directoryNotAFolder",
  "error.hooks.directoryNotAbsolute",
  "error.hooks.existingConfigInvalid",
  "error.hooks.existingConfigRootNotObject",
  "error.hooks.existingEventNotArray",
  "error.hooks.existingHooksNotObject",
  "error.hooks.existingReadFailed",
  "error.hooks.foreignFileRejected",
  "error.hooks.generatedConfigInvalid",
  "error.hooks.generatedConfigRootNotObject",
  "error.hooks.generatedEventNotArray",
  "error.hooks.generatedHooksMissing",
  "error.hooks.homeDirectoryUnavailable",
  "error.hooks.kimiManagedBlockCorrupted",
  "error.hooks.kimiMissingCommand",
  "error.hooks.kimiMissingHandler",
  "error.hooks.kimiSerializeFailed",
  "error.hooks.locationNotFound",
  "error.hooks.mergeFailed",
  "error.hooks.renderFailed",
  "error.hooks.toolUnavailable",
  "error.hooks.writeFailed",
  "error.hooks.wslUnsupportedByWindowsHost",
  "error.hooks.wslWindowsHostOnly",
  "error.monitor.behaviorDuplicate",
  "error.monitor.behaviorsIncomplete",
  "error.monitor.imageEmpty",
  "error.monitor.imageInUse",
  "error.monitor.imageNotFound",
  "error.monitor.imageUnsupportedType",
  "error.monitor.imagesInvalid",
  "error.monitor.imagesReadFailed",
  "error.monitor.imagesWriteFailed",
  "error.monitor.profilesInvalid",
  "error.monitor.profilesReadFailed",
  "error.monitor.profilesWriteFailed",
  "error.monitor.relayStatusUnavailable",
  "error.monitor.slotOutOfRange",
  "error.monitor.settingsInvalid",
  "error.monitor.settingsReadFailed",
  "error.monitor.settingsWriteFailed",
  "error.monitor.unknownImage",
]);

/** 判断 IPC 拒绝值是否是后端约定的结构化错误。 */
function structuredRustError(error: unknown): { code: string } | null {
  if (
    typeof error !== "object" ||
    error === null ||
    Array.isArray(error) ||
    !("code" in error) ||
    typeof error.code !== "string"
  ) {
    return null;
  }
  return { code: error.code };
}

/**
 * 把任意 Tauri 或运行时错误折叠为当前语言的安全说明。
 *
 * 白名单结构化错误只使用错误码查表；后端携带的 path、detail 等参数不会渲染，
 * 避免系统路径、堆栈或配置内容进入界面。未知错误继续使用固定兜底文案。
 */
export function visibleErrorMessage(error: unknown, fallback?: string): string {
  const structured = structuredRustError(error);
  if (
    structured &&
    VISIBLE_RUST_ERROR_CODES.has(structured.code) &&
    appI18n.exists(structured.code)
  ) {
    return appI18n.t(structured.code);
  }
  return fallback ?? appI18n.t("common.unknownError");
}
