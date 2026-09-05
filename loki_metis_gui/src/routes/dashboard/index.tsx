import { createFileRoute } from "@tanstack/react-router";

import { OverviewPage } from "../../pages/OverviewPage";

/** 看板概览子页。 */
export const Route = createFileRoute("/dashboard/")({
  component: OverviewPage,
});
