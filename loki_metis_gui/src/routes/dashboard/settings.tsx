import { createFileRoute } from "@tanstack/react-router";

import { DashboardSettingsSection } from "../../pages/DashboardSettingsSection";

/** 看板配置子页，承接页头「看板设置」入口。 */
export const Route = createFileRoute("/dashboard/settings")({
  component: DashboardSettingsSection,
});
