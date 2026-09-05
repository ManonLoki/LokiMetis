import { createFileRoute } from "@tanstack/react-router";

import { ChartsPage } from "../../pages/ChartsPage";

/** 看板图表子页：趋势图与用量分布合并在同一出口。 */
export const Route = createFileRoute("/dashboard/charts")({
  component: ChartsPage,
});
