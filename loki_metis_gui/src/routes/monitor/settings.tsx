import { createFileRoute } from "@tanstack/react-router";

import { MonitorSettingsPage } from "../../pages/MonitorSettingsPage";

/** 监控设置子页，不是侧栏 /settings。 */
export const Route = createFileRoute("/monitor/settings")({
  component: MonitorSettingsPage,
});
