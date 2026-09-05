import { createFileRoute } from "@tanstack/react-router";

import { CallsPage } from "../../pages/CallsPage";

/** 看板「全部」调用子页。 */
export const Route = createFileRoute("/dashboard/calls")({
  component: CallsPage,
});
