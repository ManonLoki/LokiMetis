import {
  Alert,
  Badge,
  Button,
  Group,
  NumberInput,
  Paper,
  Stack,
  Title,
} from "@mantine/core";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { useAtomValue } from "jotai";
import { useTranslation } from "react-i18next";

import { getPrivacySettings, setScanInterval } from "../api/usage";
import { synchronizeGlobalPrivacySettings } from "../api/usage-queries";
import {
  boundedMinutesNumberInputProps,
  isValidBoundedMinutes,
  parseBoundedMinutes,
} from "../bounded-minutes";
import { FailureState, LoadingState } from "../components/UsageUi";
import { agentClientAtom } from "../state/agent-client";
import { EnabledAgentsSettings } from "./EnabledAgentsSettings";
import { RetentionDaysSettings } from "./RetentionDaysSettings";
import { useDraftValue } from "../use-draft-value";
import { visibleErrorMessage } from "../visible-error";

/** 定义统一扫描间隔设置的值域与保存交互。 */
interface ScanIntervalSettingsProps {
  /** 后端当前生效的整数分钟，组件只在首次挂载时作为编辑基线。 */
  savedMinutes: number;
}

/** 展示并保存本机周期扫描间隔。 */
function ScanIntervalSettings({ savedMinutes }: ScanIntervalSettingsProps) {
  const { t } = useTranslation();
  const client = useAtomValue(agentClientAtom);
  const queryClient = useQueryClient();

  const {
    value: intervalInput,
    setDraft: setIntervalDraft,
    resetTo: resetIntervalDraft,
  } = useDraftValue<number, number | string>(savedMinutes, () => savedMinutes);
  const parsedMinutes = parseBoundedMinutes(intervalInput);
  const intervalValid = isValidBoundedMinutes(intervalInput);
  const intervalChanged = intervalValid && parsedMinutes !== savedMinutes;

  const intervalMutation = useMutation({
    mutationFn: ({
      minutes,
      targetClient,
    }: {
      minutes: number;
      targetClient: typeof client;
    }) => setScanInterval(targetClient, minutes),
    onSuccess: (settings, { targetClient }) => {
      const nextMinutes = settings.scanIntervalMinutes;
      resetIntervalDraft(nextMinutes, nextMinutes);
      queryClient.setQueryData(["privacy-settings", targetClient], settings);
      synchronizeGlobalPrivacySettings(queryClient, settings);
      void queryClient.invalidateQueries({ queryKey: ["scan-status", targetClient] });
    },
  });

  return (
    <Paper className="privacy-card" p="lg" radius="lg" withBorder>
      <form
        onSubmit={(event) => {
          event.preventDefault();
          if (intervalChanged) {
            intervalMutation.mutate({ minutes: parsedMinutes, targetClient: client });
          }
        }}
      >
        <Stack gap="md">
          <div>
            <Group gap="xs">
              <Title order={3}>{t("privacy.scanInterval.title")}</Title>
              <Badge color="blue" variant="light">
                {t("privacy.scanInterval.badge")}
              </Badge>
            </Group>
          </div>

          <NumberInput
            {...boundedMinutesNumberInputProps}
            description={t("privacy.scanInterval.description")}
            error={intervalValid ? null : t("privacy.scanInterval.error")}
            label={t("privacy.scanInterval.label")}
            onChange={(value) => {
              setIntervalDraft(value);
              intervalMutation.reset();
            }}
            value={intervalInput}
          />

          <Group justify="flex-end">
            <Button
              disabled={!intervalChanged}
              loading={intervalMutation.isPending}
              type="submit"
            >
              {t("privacy.scanInterval.save")}
            </Button>
          </Group>

          {intervalMutation.isSuccess ? (
            <Alert
              aria-live="polite"
              color="green"
              title={t("privacy.scanInterval.successTitle")}
            >
              {t("privacy.scanInterval.successBody", {
                minutes: intervalMutation.data.scanIntervalMinutes,
              })}
            </Alert>
          ) : null}
          {intervalMutation.isError ? (
            <Alert color="red" title={t("privacy.scanInterval.errorTitle")}>
              {visibleErrorMessage(intervalMutation.error)}
            </Alert>
          ) : null}
        </Stack>
      </form>
    </Paper>
  );
}

/** 在看板配置面展示 Agent 开关、扫描间隔与自动清理。 */
export function DashboardSettingsSection() {
  const { t } = useTranslation();
  const client = useAtomValue(agentClientAtom);
  const privacyQuery = useQuery({
    queryFn: () => getPrivacySettings(client),
    queryKey: ["privacy-settings", client],
  });
  if (privacyQuery.isPending) {
    return <LoadingState label={t("privacy.page.loading")} />;
  }
  if (privacyQuery.isError) {
    return (
      <FailureState
        error={privacyQuery.error}
        onRetry={() => void privacyQuery.refetch()}
      />
    );
  }

  const settings = privacyQuery.data;

  return (
    <Stack data-testid="dashboard-settings" gap="xl">
      <EnabledAgentsSettings
        availableAiTypes={settings.availableAiTypes}
        savedAgents={settings.enabledAgents}
        savedWorkbuddyStatsEnabled={settings.workbuddyStatsEnabled}
      />

      <ScanIntervalSettings
        key={`scan-interval-${client}`}
        savedMinutes={settings.scanIntervalMinutes}
      />

      <RetentionDaysSettings
        key={`retention-days-${client}`}
        savedDays={settings.retentionDays}
      />
    </Stack>
  );
}
