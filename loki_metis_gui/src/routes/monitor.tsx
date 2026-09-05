import { createFileRoute } from "@tanstack/react-router";

import { MonitorLayout } from "../components/MonitorLayout";

/** 监控布局路由：页头与子页出口。 */
export const Route = createFileRoute("/monitor")({
  component: MonitorLayout,
});
