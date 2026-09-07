import { createFileRoute } from "@tanstack/react-router";

import { UsageSettingsPage } from "../../pages/UsageSettingsPage";

/** 用量看板设置子页：集中承载扫描间隔与自动清理。 */
export const Route = createFileRoute("/dashboard/settings")({
  component: UsageSettingsPage,
});
