import { createFileRoute } from "@tanstack/react-router";

import { PetSettingsPage } from "../pages/PetSettingsPage";

/** 桌宠设置的独立原生窗口路由。 */
export const Route = createFileRoute("/pet-settings")({
  component: PetSettingsPage,
});
