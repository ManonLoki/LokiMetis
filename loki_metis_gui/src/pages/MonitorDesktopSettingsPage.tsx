import { Alert, Group, Paper, Stack, Switch, Text, Title } from "@mantine/core";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { useTranslation } from "react-i18next";

import {
  closePetOverlay,
  getMonitorSettings,
  isPetOverlayOpen,
  openPetOverlay,
  savePetCloseControlVisible,
} from "../api/monitor";

/** 监控设置页上的单个语义开关，动作只绑在 Switch 本身。 */
function MonitorPreferenceSwitch({
  checked,
  description,
  disabled,
  id,
  onChange,
  title,
}: {
  checked: boolean;
  description: string;
  disabled: boolean;
  id: string;
  onChange: (next: boolean) => void;
  title: string;
}) {
  const titleId = `${id}-title`;
  const descriptionId = `${id}-description`;
  return (
    <Paper p="lg" radius="lg" withBorder>
      <Group align="flex-start" justify="space-between" wrap="nowrap">
        <Stack gap={4} style={{ flex: 1 }}>
          <Text fw={600} id={titleId}>
            {title}
          </Text>
          <Text c="dimmed" id={descriptionId} size="sm">
            {description}
          </Text>
        </Stack>
        <Switch
          aria-describedby={descriptionId}
          aria-labelledby={titleId}
          checked={checked}
          disabled={disabled}
          onChange={(event) => {
            onChange(event.currentTarget.checked);
          }}
        />
      </Group>
    </Paper>
  );
}

/** 监控设置：桌宠悬浮窗开关与兔耳开关，不含 Hooks。 */
export function MonitorDesktopSettingsPage() {
  const { t } = useTranslation();
  const queryClient = useQueryClient();
  const overlayOpen = useQuery({
    queryFn: isPetOverlayOpen,
    queryKey: ["pet-overlay-open"],
    refetchInterval: 1000,
  });
  const settings = useQuery({ queryFn: getMonitorSettings, queryKey: ["monitor-settings"] });
  const toggleOverlay = useMutation({
    mutationFn: async (next: boolean) => {
      if (next) {
        await openPetOverlay();
      } else {
        await closePetOverlay();
      }
      return isPetOverlayOpen();
    },
    onSuccess: (open) => {
      queryClient.setQueryData(["pet-overlay-open"], open);
    },
  });
  const toggleEar = useMutation({
    mutationFn: (visible: boolean) => savePetCloseControlVisible(visible),
    onSuccess: (next) => {
      queryClient.setQueryData(["monitor-settings"], next);
    },
  });
  const petBusy = overlayOpen.isLoading || toggleOverlay.isPending;
  const earBusy = settings.isLoading || toggleEar.isPending;
  return (
    <Stack data-testid="monitor-desktop-settings" gap="md">
      <Paper p="lg" radius="lg" withBorder>
        <Stack gap="sm">
          <Title order={3}>{t("monitor.desktop.title")}</Title>
          <Text c="dimmed" size="sm">
            {t("monitor.desktop.description")}
          </Text>
        </Stack>
      </Paper>
      {toggleOverlay.error ? <Alert color="red">{String(toggleOverlay.error)}</Alert> : null}
      {toggleEar.error ? <Alert color="red">{String(toggleEar.error)}</Alert> : null}
      <MonitorPreferenceSwitch
        checked={overlayOpen.data ?? false}
        description={t("monitor.desktop.petDescription")}
        disabled={petBusy}
        id="monitor-pet-overlay"
        onChange={(next) => {
          toggleOverlay.mutate(next);
        }}
        title={t("monitor.pet.open")}
      />
      <MonitorPreferenceSwitch
        checked={settings.data?.petCloseControlVisible ?? true}
        description={t("monitor.desktop.earDescription")}
        disabled={earBusy || settings.data === undefined}
        id="monitor-pet-ear"
        onChange={(next) => {
          toggleEar.mutate(next);
        }}
        title={t("monitor.desktop.earTitle")}
      />
    </Stack>
  );
}
