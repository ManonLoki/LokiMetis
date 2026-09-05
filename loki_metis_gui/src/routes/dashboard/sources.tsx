import { createFileRoute } from "@tanstack/react-router";

import { SourcesPage } from "../../pages/SourcesPage";

/** 看板数据源子页。 */
export const Route = createFileRoute("/dashboard/sources")({
  component: SourcesPage,
});
