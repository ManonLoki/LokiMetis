import { createFileRoute } from "@tanstack/react-router";

import { MonitorImagesPage } from "../../pages/MonitorImagesPage";

/** 监控图片管理子页。 */
export const Route = createFileRoute("/monitor/images")({
  component: MonitorImagesPage,
});
