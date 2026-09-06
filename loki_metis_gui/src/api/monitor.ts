import { invoke } from "@tauri-apps/api/core";
import { open } from "@tauri-apps/plugin-dialog";

/** 与 AIMonitor Hook 目录一致的全部 Agent。 */
export type MonitorAiTool =
  | "codex"
  | "claudeCode"
  | "cursor"
  | "openCode"
  | "workBuddy"
  | "hermes"
  | "openClaw"
  | "codeBuddy"
  | "qwenCode"
  | "kimiCode"
  | "qoder"
  | "geminiCli"
  | "gitHubCopilot"
  | "grok";

/** 配置目录对象沿用 Rust 字段名的 camelCase；Copilot 键与枚举值大小写不同。 */
export type MonitorHookDirectoryKey =
  Exclude<MonitorAiTool, "gitHubCopilot"> | "githubCopilot";

/** 工具目录条目。 */
export interface MonitorAiToolDescriptor {
  tool: MonitorAiTool;
  name: string;
}

/** 前端控件使用的闭区间。 */
export interface MonitorCapabilityRange {
  default: number;
  min: number;
  max: number;
}

/** 图片上传 accept 策略。 */
export interface ImageUploadAccept {
  mimeTypes: string[];
  extensions: string[];
}

/** 监控静态能力。 */
export interface MonitorCapabilities {
  aiTools: MonitorAiToolDescriptor[];
  hookBehaviors: MonitorHookBehavior[];
  profileSlot: MonitorCapabilityRange;
  imageUploadAccept: ImageUploadAccept;
}

/** 桌宠浮窗物理像素位置。 */
export interface PetOverlayPosition {
  x: number;
  y: number;
}

/** 监控设置。 */
export interface MonitorSettings {
  enabledAiTools: MonitorAiTool[];
  hookDirectories: Partial<Record<MonitorHookDirectoryKey, string>>;
  petOverlayPosition: PetOverlayPosition | null;
}

/** Hook 配置定位。 */
export interface HookConfigLocation {
  tool: MonitorAiTool;
  directory: string;
  configPath: string;
  isCustom: boolean;
}

/** Hook 写入结果。 */
export type MonitorHookWriteOutcome =
  | "unchanged"
  | "active"
  | "restartRequired"
  | "codexReviewRequired"
  | "workBuddyReviewRequired"
  | "codeBuddyReviewRequired"
  | "hermesEnableRequired"
  | "openClawEnableRequired";

/** Hook 写入结果。 */
export interface HookConfigWriteResult {
  tool: MonitorAiTool;
  filename: string;
  outcome: MonitorHookWriteOutcome;
  configChanged: boolean;
  requiresReview: boolean;
  restartRequired: boolean;
}

/** 本机中继状态。 */
export interface HookRelayStatus {
  listening: boolean;
  bindAddress: string;
  receivedCount: number;
  failedCount: number;
  lastEvent: { tool: MonitorAiTool; hookType: string } | null;
  lastError: string | null;
}

/** 本机图库允许的格式。 */
export type MonitorImageFormat = "jpeg" | "png" | "gif";

/** 一张带预览的本机图。 */
export interface MonitorImagePreview {
  id: string;
  filename: string;
  format: MonitorImageFormat;
  image: string;
}

/** 按格式统计的图库数量。 */
export interface MonitorImageCounts {
  jpeg: number;
  png: number;
  gif: number;
}

/** 本机图库快照。 */
export interface MonitorImageGallery {
  images: MonitorImagePreview[];
  counts: MonitorImageCounts;
}

/** 展示行为。 */
export type MonitorHookBehavior = "idle" | "running" | "asking" | "error";

/** 某一行为的文案与本机图片 ID。 */
export interface MonitorHookContent {
  behavior: MonitorHookBehavior;
  content: string;
  image: string;
}

/** 一个 Agent 的展示草稿。 */
export interface MonitorProfileDraft {
  tool: MonitorAiTool;
  slot: number;
  hooks: MonitorHookContent[];
}

/** 一次读取到的完整草稿集合。 */
export interface MonitorProfileDraftSet {
  drafts: MonitorProfileDraft[];
}

/** 读取监控静态能力。 */
export async function getMonitorCapabilities(): Promise<MonitorCapabilities> {
  return invoke("get_monitor_capabilities");
}

/** 读取监控设置。 */
export async function getMonitorSettings(): Promise<MonitorSettings> {
  return invoke("get_monitor_settings");
}

/** 保存启用的 Agent。 */
export async function saveMonitorEnabledTools(
  tools: MonitorAiTool[],
): Promise<MonitorSettings> {
  return invoke("save_monitor_enabled_tools", { tools });
}

/** 列出 Hook 配置定位。 */
export async function listMonitorHookLocations(): Promise<HookConfigLocation[]> {
  return invoke("list_monitor_hook_locations");
}

/** 保存某个工具的 Hook 配置目录；空字符串恢复默认目录。 */
export async function saveMonitorHookDirectory(
  tool: MonitorAiTool,
  directory: string,
): Promise<HookConfigLocation> {
  return invoke("save_hook_config_directory", { tool, directory });
}

/** 选择本机或 Windows 可见的 WSL Hook 配置目录。 */
export async function chooseMonitorHookDirectory(
  defaultPath: string,
  title: string,
): Promise<string | null> {
  return open({
    defaultPath,
    directory: true,
    multiple: false,
    title,
  });
}

/** 写入指定工具的本机 Hook 配置。 */
export async function writeMonitorHookConfig(
  tool: MonitorAiTool,
): Promise<HookConfigWriteResult> {
  return invoke("write_monitor_hook_config", { tool });
}

/** 读取本机 Hook 中继状态。 */
export async function getHookRelayStatus(): Promise<HookRelayStatus> {
  return invoke("get_hook_relay_status");
}

/** 列出本机监控图库。 */
export async function listMonitorImages(): Promise<MonitorImageGallery> {
  return invoke("list_monitor_images_cmd");
}

/** 保存本机监控图片并返回更新后的图库。 */
export async function saveMonitorImage(
  filename: string,
  bytes: number[],
): Promise<MonitorImageGallery> {
  return invoke("save_monitor_image_cmd", { filename, bytes });
}

/** 删除本机监控图片并返回更新后的图库。 */
export async function deleteMonitorImage(id: string): Promise<MonitorImageGallery> {
  return invoke("delete_monitor_image_cmd", { id });
}

/** 读取本机展示草稿。 */
export async function listMonitorProfileDrafts(): Promise<MonitorProfileDraftSet> {
  return invoke("list_monitor_profile_drafts");
}

/** 保存一个 Agent 的展示草稿。 */
export async function saveMonitorProfileDraft(
  profile: MonitorProfileDraft,
): Promise<MonitorProfileDraft> {
  return invoke("save_monitor_profile_draft", { profile });
}

/** 桌宠当前页的排列方式。 */
export type PetLayout = "single" | "row" | "column" | "row3" | "column3" | "grid";

/** 桌宠翻页方向。 */
export type PetPageDirection = "previous" | "next";

/** 桌宠尺寸调整意图。 */
export type PetResizeDirection = "grow" | "shrink";

/** 位置中当前实际展示的 Agent 内容。 */
export interface PetOverlayTile {
  tool: MonitorAiTool;
  name: string;
  content: string;
  imageId: string | null;
}

/** 桌宠当前页的纯位置槽，空位置不预绑定 Agent。 */
export interface PetOverlaySlot {
  slotIndex: number;
  tile: PetOverlayTile | null;
}

/** 桌宠内容、分页与窗口偏好的一致快照。 */
export interface PetWindowState {
  layout: PetLayout;
  locked: boolean;
  pageIndex: number;
  pageCount: number;
  pageHasImage: boolean;
  hasAnyImage: boolean;
  slots: PetOverlaySlot[];
  petSize: number;
  sizeMin: number;
  sizeMax: number;
  alwaysOnTop: boolean;
}

/** 读取桌宠内容与窗口偏好投影。 */
export async function getPetWindowState(): Promise<PetWindowState> {
  return invoke("get_pet_window_state");
}

/** 读取监控图片原始字节。 */
export async function getMonitorImageBytes(id: string): Promise<number[]> {
  return invoke("get_monitor_image_bytes", { id });
}

/** 关闭桌宠悬浮窗。 */
export async function closePetOverlay(): Promise<void> {
  await invoke("close_pet_overlay");
}

/** 开始拖动桌宠悬浮窗。 */
export async function startPetOverlayDrag(): Promise<void> {
  await invoke("start_pet_overlay_drag");
}

/** 在桌宠所在显示器打开独立设置窗。 */
export async function showPetSettings(): Promise<void> {
  await invoke("show_pet_settings");
}

/** 隐藏独立桌宠设置窗。 */
export async function hidePetSettings(): Promise<void> {
  await invoke("hide_pet_settings");
}

/** 切换桌宠当前页排列。 */
export async function setPetLayout(layout: PetLayout): Promise<void> {
  await invoke("set_pet_layout", { layout });
}

/** 设置桌宠单格逻辑像素尺寸。 */
export async function setPetSize(size: number): Promise<void> {
  await invoke("set_pet_size", { size });
}

/** 设置桌宠是否始终置顶。 */
export async function setPetAlwaysOnTop(enabled: boolean): Promise<void> {
  await invoke("set_pet_always_on_top", { enabled });
}

/** 设置桌宠是否锁定位置与大小。 */
export async function setPetLocked(locked: boolean): Promise<void> {
  await invoke("set_pet_locked", { locked });
}

/** 按宿主管理的分页状态切换桌宠页。 */
export async function turnPetPage(direction: PetPageDirection): Promise<void> {
  await invoke("turn_pet_page", { direction });
}

/** 在每次 WebView 挂载时至多一次聚焦首个有图页。 */
export async function focusFirstPopulatedPetPage(): Promise<void> {
  await invoke("focus_first_populated_pet_page");
}

/** 上报单步放大或缩小意图，步长与边界由宿主决定。 */
export async function resizePetStep(direction: PetResizeDirection): Promise<void> {
  await invoke("resize_pet_step", { direction });
}

/** 显示并聚焦主界面。 */
export async function showMainWindow(): Promise<void> {
  await invoke("show_main_window");
}

/** 把选中的本地文件读成 IPC 字节数组。 */
export async function fileBytes(file: File): Promise<number[]> {
  return Array.from(new Uint8Array(await file.arrayBuffer()));
}

/** 把 Rust 上传策略映射为 file input 的 accept 值。 */
export function imageUploadAcceptValue(policy: ImageUploadAccept | undefined): string {
  return policy ? [...policy.extensions, ...policy.mimeTypes].join(",") : "";
}
