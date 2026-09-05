import { Stack } from "@mantine/core";
import { Outlet } from "@tanstack/react-router";
import type { ReactElement } from "react";

import { MonitorToolbar } from "./MonitorToolbar";

/** 装配监控页头与子页出口。 */
export function MonitorLayout(): ReactElement {
  return (
    <Stack data-testid="monitor-page" gap="md">
      <MonitorToolbar />
      <Outlet />
    </Stack>
  );
}
