import { invoke } from "@tauri-apps/api/core";

/** 监控区支持的四项 Agent。 */
export type MonitorAiTool = "codex" | "claudeCode" | "grok" | "workBuddy";

/** 工具目录条目。 */
export interface MonitorAiToolDescriptor {
  tool: MonitorAiTool;
  name: string;
}

/** 监控静态能力。 */
export interface MonitorCapabilities {
  aiTools: MonitorAiToolDescriptor[];
  hookBehaviors: string[];
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

/** 本机监控图片。 */
export interface MonitorImageRecord {
  id: string;
  filename: string;
  storedName: string;
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

/** 列出本机监控图片。 */
export async function listMonitorImages(): Promise<MonitorImageRecord[]> {
  return invoke("list_monitor_images_cmd");
}

/** 保存本机监控图片。 */
export async function saveMonitorImage(
  filename: string,
  bytes: number[],
): Promise<MonitorImageRecord> {
  return invoke("save_monitor_image_cmd", { filename, bytes });
}

/** 删除本机监控图片。 */
export async function deleteMonitorImage(id: string): Promise<void> {
  return invoke("delete_monitor_image_cmd", { id });
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
