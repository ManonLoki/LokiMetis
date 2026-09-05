import { createFileRoute } from "@tanstack/react-router";

import { MonitorDesktopSettingsPage } from "../../pages/MonitorDesktopSettingsPage";

/** 监控设置子页：桌宠悬浮窗与兔耳，不是 Hooks 设置也不是应用 /settings。 */
export const Route = createFileRoute("/monitor/desktop")({
  component: MonitorDesktopSettingsPage,
});
