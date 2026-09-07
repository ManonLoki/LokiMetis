import { createFileRoute } from "@tanstack/react-router";

import { SkinPage } from "../pages/SkinPage";

export const Route = createFileRoute("/skins")({
  component: SkinPage,
});
