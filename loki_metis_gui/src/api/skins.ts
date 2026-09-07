/**
 * 集中定义换皮页面与 Tauri 宿主之间的类型化 IPC；页面组件不直接调用 `invoke`。
 */
import { Channel, invoke, isTauri } from "@tauri-apps/api/core";
import { getCurrentWebview, type DragDropEvent } from "@tauri-apps/api/webview";

/** 皮肤来源；内置资源只读，用户资源可管理。 */
export type SkinSource = "builtin" | "user";
/** 皮肤包类型；纯主题不携带自由注入脚本。 */
export type SkinPackageType = "legacySkin" | "theme";
/** Codex 浅色或深色外观。 */
export type ColorMode = "light" | "dark";
/** Codex GUI 与回环调试端点的可观察状态。 */
export type CodexRuntimeState = "stopped" | "ready" | "runningWithoutCdp";
/** 宿主兼容规则的聚合结果。 */
export type SkinCompatibilityMode = "native" | "adapted" | "partial";

/** 精确引用一个内置或用户皮肤。 */
export interface SkinReference {
  source: SkinSource;
  id: string;
}

/** 描述一个经过原生层完整校验的皮肤。 */
export interface SkinDescriptor extends SkinReference {
  packageType: SkinPackageType;
  name: string;
  author: string;
  version: string;
  previewDataUrl: string;
  supportedColorModes: ColorMode[];
}

/** 描述当前运行中的皮肤宿主兼容情况。 */
export interface SkinCompatibilityStatus {
  version: string;
  mode: SkinCompatibilityMode;
  appliedRules: string[];
  skippedRules: string[];
}

/** 描述当前或最后目标实例的注入状态。 */
export interface SkinStatus {
  installed: boolean;
  skinId: string | null;
  source: SkinSource | null;
  packageType: SkinPackageType | null;
  skinName: string | null;
  version: string;
  affectedPages: number;
  compatibility: SkinCompatibilityStatus | null;
  instanceId?: string;
}

/** 描述 Codex 是否可直接连接。 */
export interface CodexRuntimeStatus {
  state: CodexRuntimeState;
}

/** 描述一个已验证的 Codex GUI 主进程和可选账户资料。 */
export interface CodexInstance {
  id: string;
  pid: number;
  label: string;
  profile: string | null;
  state: Exclude<CodexRuntimeState, "stopped">;
  debugPort: number | null;
  activeSkinName: string | null;
  activeSkin: SkinReference | null;
  accountLabel: string | null;
  avatarDataUrl: string | null;
}

/** 描述外观确认中一个可读字段差异。 */
export interface AppearanceDifference {
  field: string;
  label: string;
  currentValue: string | null;
  expectedValue: string;
}

/** 描述应用前需要用户确认的外观差异。 */
export interface SkinAppearanceCheck {
  effectiveMode: ColorMode;
  supportedColorModes: ColorMode[];
  differences: AppearanceDifference[];
  unreadable: AppearanceDifference[];
}

/** 描述应用成功或需要显式外观确认两种结果。 */
export type InstallSkinResult =
  | { type: "installed"; status: SkinStatus }
  | { type: "needsConfirmation"; check: SkinAppearanceCheck };

/** 描述应用内生成主题时交给 Codex 的动态提示词。 */
export interface SkinCreationPrompt {
  prompt: string;
}

/** 描述旧兼容皮肤转换后的主题与回退配色。 */
export interface ThemeConversionResult {
  theme: SkinDescriptor;
  fallbackRoles: string[];
}

/** 描述批量删除中的单项失败。 */
export interface FailedSkinDelete {
  skin: SkinReference;
  code: string;
  message: string;
}

/** 描述批量删除的成功与失败集合。 */
export interface BatchDeleteResult {
  deleted: SkinReference[];
  failed: FailedSkinDelete[];
}

/** 描述一个已经安全解压并校验、等待确认的导入项。 */
export interface PreparedSkinImportItem {
  itemId: string;
  archiveName: string;
  skin: SkinDescriptor;
}

/** 描述导入预检中被跳过的压缩包。 */
export interface SkippedSkinImport {
  archiveName: string;
  code: string;
  message: string;
  details: string[];
}

/** 描述一个有 owner、可提交或取消的导入预检批次。 */
export interface PreparedSkinImportBatch {
  token: string;
  items: PreparedSkinImportItem[];
  totalFiles: number;
  skipped: SkippedSkinImport[];
}

/** 描述提交导入时某个已选项的失败。 */
export interface FailedSkinImport {
  itemId: string;
  archiveName: string;
  skinName: string;
  code: string;
  message: string;
  details: string[];
}

/** 描述批量导入最终结果。 */
export interface BatchImportResult {
  installed: SkinDescriptor[];
  failed: FailedSkinImport[];
  skippedCount: number;
}

/** 描述导入预检的有界增量事件。 */
export type SkinImportPreparationEvent =
  | { type: "started"; token: string; totalFiles: number }
  | { type: "itemReady"; token: string; item: PreparedSkinImportItem }
  | { type: "itemSkipped"; token: string; item: SkippedSkinImport };

/** 标准化换皮 IPC 返回的结构化错误。 */
export class SkinHostError extends Error {
  readonly code: string;
  readonly details: string[];

  /** 保存稳定错误码、脱敏消息和可选详情。 */
  constructor(code: string, message: string, details: string[] = []) {
    super(message);
    this.name = "SkinHostError";
    this.code = code;
    this.details = details;
  }
}

/** 把未知 Tauri 拒绝值收敛为稳定前端错误。 */
function normalizeSkinError(error: unknown): SkinHostError {
  if (typeof error === "object" && error !== null) {
    const candidate = error as Record<string, unknown>;
    if (typeof candidate.code === "string" && typeof candidate.message === "string") {
      const details = Array.isArray(candidate.details)
        ? candidate.details.filter((item): item is string => typeof item === "string")
        : [];
      return new SkinHostError(candidate.code, candidate.message, details);
    }
  }
  if (error instanceof Error) return new SkinHostError("skin.unknown", error.message);
  return new SkinHostError("skin.unknown", String(error));
}

/** 调用换皮命令，并统一错误边界。 */
async function invokeSkin<TResult>(
  command: string,
  arguments_?: Record<string, unknown>,
): Promise<TResult> {
  try {
    return await invoke<TResult>(command, arguments_);
  } catch (error) {
    throw normalizeSkinError(error);
  }
}

/** 为批量预检创建类型化增量通道。 */
function progressChannel(
  onProgress: (event: SkinImportPreparationEvent) => void,
): Channel<SkinImportPreparationEvent> {
  return new Channel<SkinImportPreparationEvent>(onProgress);
}

/** 返回当前页面是否运行在真实 Tauri WebView 中。 */
export function skinHostAvailable(): boolean {
  return isTauri();
}

/** 暴露换皮页面唯一的原生 API 集合。 */
export const skinApi = {
  status: () => invokeSkin<SkinStatus>("skin_status"),
  list: () => invokeSkin<SkinDescriptor[]>("list_skins"),
  catalogChanged: () => invokeSkin<boolean>("skin_catalog_changed"),
  creationPrompt: () => invokeSkin<SkinCreationPrompt>("skin_creation_prompt"),
  createTheme: (name: string, author: string) =>
    invokeSkin<SkinDescriptor>("create_user_theme", { name, author }),
  convertToTheme: (skin: SkinReference) =>
    invokeSkin<ThemeConversionResult>("convert_skin_to_theme", { skin }),
  exportPackage: (skin: SkinReference) =>
    invokeSkin<boolean>("export_skin_package", { skin }),
  prepareImport: (onProgress: (event: SkinImportPreparationEvent) => void) =>
    invokeSkin<PreparedSkinImportBatch | null>("prepare_skin_import", {
      onProgress: progressChannel(onProgress),
    }),
  prepareDroppedPaths: (
    paths: string[],
    onProgress: (event: SkinImportPreparationEvent) => void,
  ) =>
    invokeSkin<PreparedSkinImportBatch>("prepare_skin_zip_paths", {
      paths,
      onProgress: progressChannel(onProgress),
    }),
  commitImport: (token: string, selectedItemIds: string[]) =>
    invokeSkin<BatchImportResult>("commit_skin_import", { token, selectedItemIds }),
  cancelImport: (token: string) => invokeSkin<void>("cancel_skin_import", { token }),
  openDirectory: (skin: SkinReference) => invokeSkin<void>("open_skin_directory", { skin }),
  delete: (skin: SkinReference) => invokeSkin<void>("delete_skin", { skin }),
  deleteMany: (skins: SkinReference[]) =>
    invokeSkin<BatchDeleteResult>("delete_skins", { skins }),
  runtimeStatus: () => invokeSkin<CodexRuntimeStatus>("codex_runtime_status"),
  instances: () => invokeSkin<CodexInstance[]>("list_codex_instances"),
  probeInstance: (instanceId: string) =>
    invokeSkin<CodexInstance>("probe_codex_instance", { instanceId }),
  restartInstance: (instanceId: string) =>
    invokeSkin<CodexInstance>("restart_codex_instance", { instanceId }),
  launchCodex: () => invokeSkin<CodexRuntimeStatus>("launch_codex"),
  forceLaunchCodex: () => invokeSkin<CodexRuntimeStatus>("force_launch_codex"),
  cancelCodexOperation: () => invokeSkin<boolean>("cancel_codex_operation"),
  install: (
    skin: SkinReference,
    allowAppearanceMismatch = false,
    instanceId: string | null = null,
  ) =>
    invokeSkin<InstallSkinResult>(
      "install_skin",
      instanceId === null
        ? { skin, allowAppearanceMismatch }
        : { skin, allowAppearanceMismatch, instanceId },
    ),
  uninstall: (instanceId: string | null = null) =>
    invokeSkin<SkinStatus>(
      "uninstall_skin",
      instanceId === null ? undefined : { instanceId },
    ),
  onFileDrop: async (handler: (event: DragDropEvent) => void) => {
    if (!isTauri()) return () => undefined;
    return getCurrentWebview().onDragDropEvent(({ payload }) => handler(payload));
  },
};
