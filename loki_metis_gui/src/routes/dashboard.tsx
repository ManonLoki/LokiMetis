import { createFileRoute } from "@tanstack/react-router";

import { DashboardLayout } from "../components/DashboardLayout";

/** 看板布局路由：页头与子页出口。 */
export const Route = createFileRoute("/dashboard")({
  component: DashboardLayout,
});
