import { createFileRoute } from "@tanstack/react-router";

import { MonitorManagementPage } from "../../pages/MonitorManagementPage";

/** 监控管理子页。 */
export const Route = createFileRoute("/monitor/management")({
  component: MonitorManagementPage,
});
