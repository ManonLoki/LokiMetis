import { invoke } from "@tauri-apps/api/core";

/** 监控区支持的四项 Agent。 */
export type MonitorAiTool = "codex" | "claudeCode" | "grok" | "workBuddy";

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
  hookDirectories: {
    codex: string;
    claudeCode: string;
    grok: string;
    workBuddy: string;
  };
  petCloseControlVisible: boolean;
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
export interface HookConfigWriteResult {
  tool: MonitorAiTool;
  filename: string;
  outcome: string;
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

/** 桌宠宫格槽位。 */
export interface PetOverlaySlot {
  tool: MonitorAiTool;
  name: string;
  occupied: boolean;
  imageId: string | null;
}

/** 桌宠宫格快照。 */
export interface PetOverlayView {
  slots: PetOverlaySlot[];
}

/** 读取桌宠宫格投影。 */
export async function getPetOverlayView(): Promise<PetOverlayView> {
  return invoke("get_pet_overlay_view");
}

/** 读取监控图片原始字节。 */
export async function getMonitorImageBytes(id: string): Promise<number[]> {
  return invoke("get_monitor_image_bytes", { id });
}

/** 查询桌宠悬浮窗当前是否打开。 */
export async function isPetOverlayOpen(): Promise<boolean> {
  return invoke("is_pet_overlay_open");
}

/** 保存兔耳（圆形关闭控件）显示偏好。 */
export async function savePetCloseControlVisible(
  visible: boolean,
): Promise<MonitorSettings> {
  return invoke("save_pet_close_control_visible", { visible });
}

/** 打开桌宠悬浮窗。 */
export async function openPetOverlay(): Promise<void> {
  await invoke("open_pet_overlay");
}

/** 关闭桌宠悬浮窗。 */
export async function closePetOverlay(): Promise<void> {
  await invoke("close_pet_overlay");
}

/** 开始拖动桌宠悬浮窗。 */
export async function startPetOverlayDrag(): Promise<void> {
  await invoke("start_pet_overlay_drag");
}

/** 从宿主读回当前浮窗位置。 */
export async function getPetOverlayPosition(): Promise<PetOverlayPosition> {
  return invoke("get_pet_overlay_position");
}

/** 保存浮窗位置；坐标由宿主读回后再交给本机规范化。 */
export async function savePetOverlayPosition(
  position: PetOverlayPosition,
): Promise<MonitorSettings> {
  return invoke("save_pet_overlay_position", { position });
}

/** 把 Rust 上传策略映射为 file input 的 accept 值。 */
export function imageUploadAcceptValue(policy: ImageUploadAccept | undefined): string {
  return policy ? [...policy.extensions, ...policy.mimeTypes].join(",") : "";
}
