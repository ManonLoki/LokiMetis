import { createFileRoute } from "@tanstack/react-router";

import { MonitorWorkbenchPage } from "../../pages/MonitorWorkbenchPage";

/** 监控工作台子页。 */
export const Route = createFileRoute("/monitor/")({
  component: MonitorWorkbenchPage,
});
