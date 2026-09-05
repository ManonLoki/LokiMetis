import { createFileRoute } from "@tanstack/react-router";

import { PetOverlayPage } from "../pages/PetOverlayPage";

/** 桌宠悬浮窗独立路由，不挂主壳。 */
export const Route = createFileRoute("/pet")({
  component: PetOverlayPage,
});
