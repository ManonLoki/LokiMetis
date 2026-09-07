import { createFileRoute } from "@tanstack/react-router";

import { MonitorSettingsPage } from "../../pages/MonitorSettingsPage";

/** 监控 Hooks 设置子页，不是侧栏公共设置页。 */
export const Route = createFileRoute("/monitor/settings")({
  component: MonitorSettingsPage,
});
