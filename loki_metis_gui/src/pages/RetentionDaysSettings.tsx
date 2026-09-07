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
import { useMutation, useQueryClient } from "@tanstack/react-query";
import { useAtomValue } from "jotai";
import { useTranslation } from "react-i18next";

import { setRetentionDays } from "../api/usage";
import { synchronizeGlobalPrivacySettings } from "../api/usage-queries";
import { parseBoundedMinutes } from "../bounded-minutes";
import { agentClientAtom } from "../state/agent-client";
import { useDraftValue } from "../use-draft-value";
import { visibleErrorMessage } from "../visible-error";

/** 定义自动清理天数设置的草稿和值域错误。 */
interface RetentionDaysSettingsProps {
  /** 后端当前生效的整数天，组件只在首次挂载时作为编辑基线。 */
  savedDays: number;
}

const RETENTION_DAYS_MIN = 1;
const RETENTION_DAYS_MAX = 3_650;

/** 展示并保存派生用量自动清理天数；保存不立刻删数据。 */
export function RetentionDaysSettings({ savedDays }: RetentionDaysSettingsProps) {
  const { t } = useTranslation();
  const client = useAtomValue(agentClientAtom);
  const queryClient = useQueryClient();
  const {
    value: daysInput,
    setDraft: setDaysDraft,
    resetTo: resetDaysDraft,
  } = useDraftValue<number, number | string>(savedDays, () => savedDays);
  const parsedDays = parseBoundedMinutes(daysInput);
  const daysValid =
    Number.isInteger(parsedDays) &&
    parsedDays >= RETENTION_DAYS_MIN &&
    parsedDays <= RETENTION_DAYS_MAX;
  const daysChanged = daysValid && parsedDays !== savedDays;
  const daysMutation = useMutation({
    mutationFn: ({ days, targetClient }: { days: number; targetClient: typeof client }) =>
      setRetentionDays(targetClient, days),
    onSuccess: (settings, { targetClient }) => {
      const nextDays = settings.retentionDays;
      resetDaysDraft(nextDays, nextDays);
      queryClient.setQueryData(["privacy-settings", targetClient], settings);
      synchronizeGlobalPrivacySettings(queryClient, settings);
    },
  });

  return (
    <Paper className="privacy-card" p="lg" radius="lg" withBorder>
      <form
        onSubmit={(event) => {
          event.preventDefault();
          if (daysChanged) {
            daysMutation.mutate({ days: parsedDays, targetClient: client });
          }
        }}
      >
        <Stack gap="md">
          <div>
            <Group gap="xs">
              <Title order={3}>{t("privacy.retentionDays.title")}</Title>
              <Badge color="blue" variant="light">
                {t("privacy.retentionDays.badge")}
              </Badge>
            </Group>
          </div>
          <NumberInput
            allowDecimal={false}
            allowNegative={false}
            clampBehavior="none"
            error={daysValid ? null : t("privacy.retentionDays.error")}
            label={t("privacy.retentionDays.label")}
            max={RETENTION_DAYS_MAX}
            min={RETENTION_DAYS_MIN}
            onChange={(value) => {
              setDaysDraft(value);
              daysMutation.reset();
            }}
            value={daysInput}
          />
          <Group justify="flex-end">
            <Button disabled={!daysChanged} loading={daysMutation.isPending} type="submit">
              {t("privacy.retentionDays.save")}
            </Button>
          </Group>
          {daysMutation.isSuccess ? (
            <Alert
              aria-live="polite"
              color="green"
              title={t("privacy.retentionDays.successTitle")}
            >
              {t("privacy.retentionDays.successBody", {
                days: daysMutation.data.retentionDays,
              })}
            </Alert>
          ) : null}
          {daysMutation.isError ? (
            <Alert color="red" title={t("privacy.retentionDays.errorTitle")}>
              {visibleErrorMessage(daysMutation.error)}
            </Alert>
          ) : null}
        </Stack>
      </form>
    </Paper>
  );
}
