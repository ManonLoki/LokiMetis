import { Alert, Stack } from "@mantine/core";
import { useQuery } from "@tanstack/react-query";
import { Outlet, useNavigate, useRouterState } from "@tanstack/react-router";
import { useAtom } from "jotai";
import { useEffect, useMemo, type ReactElement } from "react";
import { useTranslation } from "react-i18next";

import { getPrivacySettings, type UsageViewKind } from "../api/usage";
import { agentClientAtom, usageViewAtom } from "../state/agent-client";
import { DashboardToolbar } from "./DashboardToolbar";
import { FailureState, LoadingState } from "./UsageUi";

/** 装配看板页头、已开启 Agent 切换与子页出口。 */
export function DashboardLayout(): ReactElement {
  const { t } = useTranslation();
  const navigate = useNavigate();
  const pathname = useRouterState({ select: (state) => state.location.pathname });
  const [client, setClient] = useAtom(agentClientAtom);
  const [view, setView] = useAtom(usageViewAtom);
  const privacyQuery = useQuery({
    queryFn: () => getPrivacySettings("codex"),
    queryKey: ["privacy-settings", "codex"],
  });
  const workbuddyStatsEnabled = privacyQuery.data?.workbuddyStatsEnabled === true;
  const enabledAgents = useMemo(
    () => privacyQuery.data?.enabledAgents ?? [],
    [privacyQuery.data?.enabledAgents],
  );

  useEffect(() => {
    const firstEnabled = enabledAgents[0];
    if (view === "workbuddy") {
      if (!workbuddyStatsEnabled && firstEnabled !== undefined) {
        setView(firstEnabled);
      }
      return;
    }
    if (firstEnabled === undefined) {
      if (workbuddyStatsEnabled && view !== "all") {
        setView("workbuddy");
      }
      return;
    }
    if (!enabledAgents.includes(client)) {
      setClient(firstEnabled);
    }
    if (view !== "all" && !enabledAgents.includes(view)) {
      setView(firstEnabled);
    }
  }, [client, enabledAgents, setClient, setView, view, workbuddyStatsEnabled]);

  /** 切换物理 Agent、全部或 WorkBuddy 视图；选择只在当前桌面进程内生效。 */
  const handleViewChange = (nextView: UsageViewKind): void => {
    setView(nextView);
    if (
      nextView === "all" &&
      (pathname === "/dashboard/usage" ||
        pathname === "/dashboard/charts" ||
        pathname === "/dashboard/sources")
    ) {
      void navigate({ to: "/dashboard" });
      return;
    }
    if (nextView === "workbuddy" && pathname === "/dashboard/calls") {
      void navigate({ to: "/dashboard" });
      return;
    }
    if (nextView !== "all" && nextView !== "workbuddy") {
      setClient(nextView);
    }
  };

  if (privacyQuery.isPending) {
    return <LoadingState />;
  }
  if (privacyQuery.isError) {
    return (
      <FailureState
        error={privacyQuery.error}
        onRetry={() => void privacyQuery.refetch()}
      />
    );
  }

  return (
    <Stack data-testid="dashboard-page" gap="md">
      {enabledAgents.length === 0 && !workbuddyStatsEnabled ? (
        <Alert title={t("shell.noEnabledAgents.title")}>
          {t("shell.noEnabledAgents.body")}
        </Alert>
      ) : null}
      <DashboardToolbar
        enabledAgents={enabledAgents}
        onViewChange={handleViewChange}
        view={view}
        workbuddyStatsEnabled={workbuddyStatsEnabled}
      />
      <Outlet />
    </Stack>
  );
}
