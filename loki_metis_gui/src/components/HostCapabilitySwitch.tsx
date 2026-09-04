import { Alert, Button, Group, Paper, Stack, Switch, Text } from "@mantine/core";
import {
  useMutation,
  useQuery,
  useQueryClient,
  type QueryKey,
} from "@tanstack/react-query";
import { IconAlertCircle, IconRefresh } from "@tabler/icons-react";
import { useEffect, useState, type ReactElement } from "react";
import { useTranslation } from "react-i18next";

/** 设置页固定支持的 Rust-only 宿主能力。 */
export type HostCapabilityId = "system_notification" | "autostart";

/** 描述一个权威布尔宿主能力的读写契约。 */
export interface HostCapabilitySwitchProps {
  id: HostCapabilityId;
  queryKey: QueryKey;
  getEnabled: () => Promise<boolean>;
  setEnabled: (enabled: boolean) => Promise<boolean>;
}

/** 渲染以宿主为准、失败后回滚并可重新读取的能力开关。 */
export function HostCapabilitySwitch({
  id,
  queryKey,
  getEnabled,
  setEnabled,
}: HostCapabilitySwitchProps): ReactElement {
  const { t } = useTranslation();
  const queryClient = useQueryClient();
  const setting = useQuery({
    queryKey,
    queryFn: () => getEnabled(),
    retry: false,
  });
  const [displayed, setDisplayed] = useState<boolean | undefined>(setting.data);
  const [updateFailed, setUpdateFailed] = useState(false);
  const titleId = `capability-${id}-title`;
  const descriptionId = `capability-${id}-description`;

  useEffect(() => {
    if (setting.data !== undefined) setDisplayed(setting.data);
  }, [setting.data]);

  const update = useMutation({
    mutationFn: setEnabled,
    onMutate: (nextEnabled) => {
      setUpdateFailed(false);
      setDisplayed(nextEnabled);
    },
    onSuccess: (authoritativeEnabled) => {
      setDisplayed(authoritativeEnabled);
      queryClient.setQueryData(queryKey, authoritativeEnabled);
    },
    onError: async () => {
      const authoritative = await setting.refetch();
      setDisplayed(authoritative.data);
      setUpdateFailed(true);
    },
  });

  const unknown = setting.isError && displayed === undefined;
  const busy = setting.isLoading || setting.isFetching || update.isPending;

  return (
    <Paper data-testid={`settings-capability-${id}`} p="lg" radius="lg" withBorder>
      <Stack gap="sm">
        <Group align="flex-start" justify="space-between" wrap="nowrap">
          <Stack gap={4} style={{ flex: 1 }}>
            <Text fw={600} id={titleId}>
              {t(`settings.${id}_title`)}
            </Text>
            <Text c="dimmed" id={descriptionId} lh={1.55} size="sm">
              {t(`settings.${id}_description`)}
            </Text>
          </Stack>
          <Switch
            aria-describedby={descriptionId}
            aria-labelledby={titleId}
            checked={displayed ?? false}
            data-authoritative-state={
              displayed === undefined ? "unknown" : displayed ? "enabled" : "disabled"
            }
            disabled={busy || displayed === undefined}
            onChange={(event) => {
              update.mutate(event.currentTarget.checked);
            }}
          />
        </Group>

        {busy ? (
          <Text aria-live="polite" c="dimmed" role="status" size="sm">
            {t(
              update.isPending
                ? "settings.capability_pending"
                : "settings.capability_loading",
            )}
          </Text>
        ) : null}

        {updateFailed ? (
          <Alert
            icon={<IconAlertCircle aria-hidden="true" size={18} />}
            role="alert"
            title={t("settings.capability_error_title")}
          >
            {t(`settings.${id}_error`)}
          </Alert>
        ) : null}

        {unknown ? (
          <Alert
            icon={<IconAlertCircle aria-hidden="true" size={18} />}
            role="alert"
            title={t("settings.capability_unknown_title")}
          >
            <Stack align="flex-start" gap="sm">
              {t(`settings.${id}_unknown`)}
              <Button
                leftSection={<IconRefresh aria-hidden="true" size={16} />}
                onClick={() => {
                  setUpdateFailed(false);
                  void setting.refetch();
                }}
                size="xs"
                variant="light"
              >
                {t("settings.capability_retry")}
              </Button>
            </Stack>
          </Alert>
        ) : null}
      </Stack>
    </Paper>
  );
}
