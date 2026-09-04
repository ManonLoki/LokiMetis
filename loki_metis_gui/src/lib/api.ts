import { invoke } from "@tauri-apps/api/core";

/** 表示前端支持的固定界面语言。 */
export type InterfaceLanguage = "zh-CN" | "en-US";

/** 描述由 Tauri 安装包提供的稳定应用元数据。 */
export interface AppMetadata {
  applicationName: string;
  version: string;
  productDefinitionRequired: boolean;
  title: string;
}

/** 允许测试替换唯一 Tauri IPC 调用边界。 */
export type HostInvoker = (
  command: string,
  arguments_?: Record<string, unknown>,
) => Promise<unknown>;

const invokeHost: HostInvoker = (command, arguments_) =>
  invoke<unknown>(command, arguments_);

/** 判断未知 IPC 值是否为普通记录对象。 */
export function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}

/** 从未知记录中读取经过裁剪验证的非空字符串。 */
function readString(value: Record<string, unknown>, field: string): string {
  const candidate = value[field];
  if (typeof candidate !== "string" || candidate.trim() === "") {
    throw new Error(`invalid ${field}`);
  }
  return candidate;
}

/** 要求元数据值完整且不把不可信 IPC 内容直接渲染到页面。 */
export function decodeAppMetadata(value: unknown): AppMetadata {
  if (!isRecord(value) || typeof value.productDefinitionRequired !== "boolean") {
    throw new Error("invalid app metadata");
  }

  const applicationName = readString(value, "applicationName");
  const version = readString(value, "version");
  const title = readString(value, "title");
  if (!/^\d+\.\d+\.\d+$/u.test(version)) throw new Error("invalid version");

  return {
    applicationName,
    version,
    productDefinitionRequired: value.productDefinitionRequired,
    title,
  };
}

/** 判断未知 IPC 值是否为前端支持的规范语言。 */
export function decodeInterfaceLanguage(value: unknown): InterfaceLanguage {
  if (value !== "zh-CN" && value !== "en-US") {
    throw new Error("invalid interface language");
  }
  return value;
}

/** 读取当前安装包元数据。 */
export async function getAppMetadata(): Promise<AppMetadata> {
  return decodeAppMetadata(await invoke<unknown>("get_app_metadata"));
}

/** 允许测试替换元数据读取边界。 */
export async function getAppMetadataWith(
  invoker: HostInvoker = invokeHost,
): Promise<AppMetadata> {
  return decodeAppMetadata(await invoker("get_app_metadata"));
}

/** 读取并验证系统语言。 */
export async function getSystemLocale(): Promise<InterfaceLanguage> {
  return decodeInterfaceLanguage(await invoke<unknown>("get_system_locale"));
}

/** 同步原生菜单语言并返回宿主确认的规范语言。 */
export async function setInterfaceLanguage(
  language: InterfaceLanguage,
): Promise<InterfaceLanguage> {
  return decodeInterfaceLanguage(
    await invoke<unknown>("set_interface_language", { language }),
  );
}

/** 允许测试替换语言写入边界。 */
export async function setInterfaceLanguageWith(
  language: InterfaceLanguage,
  invoker: HostInvoker = invokeHost,
): Promise<InterfaceLanguage> {
  return decodeInterfaceLanguage(await invoker("set_interface_language", { language }));
}

/** 读取系统通知的权威启用状态。 */
export async function getSystemNotificationSetting(
  invoker: HostInvoker = invokeHost,
): Promise<boolean> {
  const value = await invoker("get_system_notification_setting");
  if (typeof value !== "boolean") throw new Error("invalid notification setting");
  return value;
}

/** 更新系统通知并返回宿主确认的权威状态。 */
export async function setSystemNotificationEnabled(
  enabled: boolean,
  invoker: HostInvoker = invokeHost,
): Promise<boolean> {
  const value = await invoker("set_system_notification_enabled", { enabled });
  if (typeof value !== "boolean") throw new Error("invalid notification setting");
  return value;
}

/** 读取操作系统登录项的权威启用状态。 */
export async function getAutostartEnabled(
  invoker: HostInvoker = invokeHost,
): Promise<boolean> {
  const value = await invoker("get_autostart_enabled");
  if (typeof value !== "boolean") throw new Error("invalid autostart setting");
  return value;
}

/** 更新操作系统登录项并返回宿主确认的权威状态。 */
export async function setAutostartEnabled(
  enabled: boolean,
  invoker: HostInvoker = invokeHost,
): Promise<boolean> {
  const value = await invoker("set_autostart_enabled", { enabled });
  if (typeof value !== "boolean") throw new Error("invalid autostart setting");
  return value;
}
