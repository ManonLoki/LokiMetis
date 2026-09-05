import { createFileRoute } from "@tanstack/react-router";

import { UsagePage } from "../../pages/UsagePage";

/** 看板用量子页。 */
export const Route = createFileRoute("/dashboard/usage")({
  component: UsagePage,
});
