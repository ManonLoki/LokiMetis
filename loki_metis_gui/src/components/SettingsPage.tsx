import {
  Alert,
  Badge,
  Button,
  Group,
  Paper,
  SegmentedControl,
  SimpleGrid,
  Skeleton,
  Stack,
  Text,
  Title,
  useMantineColorScheme,
} from "@mantine/core";
import { useMutation, useQuery } from "@tanstack/react-query";
import { IconAlertCircle, IconHistory } from "@tabler/icons-react";
import { useAtom } from "jotai";
import { useState, type ReactElement } from "react";
import { useTranslation } from "react-i18next";

import {
  getAutostartEnabled,
  getSystemNotificationSetting,
  setAutostartEnabled,
  setSystemNotificationEnabled,
  type InterfaceLanguage,
} from "../lib/api";
import { persistInterfaceLanguage } from "../lib/language";
import { appMetadataQuery, autostartQuery, systemNotificationQuery } from "../lib/queries";
import {
  formatDisplayVersion,
  loadBundledReleaseNotes,
  resolveReleaseNotesLocale,
  type ReleaseNotesDocument,
} from "../lib/releaseNotes";
import { interfaceLanguageAtom } from "../state/interfaceLanguage";
import type { AppColorScheme } from "./AppThemeProvider";
import { syncShellInterfaceLanguage } from "./AppShell";
import { HostCapabilitySwitch } from "./HostCapabilitySwitch";
import { ReleaseNotesDialogTemplate, type ReleaseNotesStatus } from "./ReleaseNotesDialog";

/** 描述设置页可替换的本地更新日志加载边界。 */
export interface SettingsPageProps {
  releaseNotesLoader?: () => Promise<ReleaseNotesDocument>;
}

/** 判断分段控件值是否为固定界面语言。 */
function isInterfaceLanguage(value: string): value is InterfaceLanguage {
  return value === "zh-CN" || value === "en-US";
}

/** 判断分段控件值是否为固定主题偏好。 */
function isAppColorScheme(value: string): value is AppColorScheme {
  return value === "light" || value === "dark" || value === "auto";
}

/** 渲染应用信息、本地偏好、更新日志及已启用宿主能力。 */
export function SettingsPage({
  releaseNotesLoader = loadBundledReleaseNotes,
}: SettingsPageProps = {}): ReactElement {
  const { i18n, t } = useTranslation();
  const metadata = useQuery(appMetadataQuery);
  const [language, setLanguage] = useAtom(interfaceLanguageAtom);
  const { colorScheme, setColorScheme } = useMantineColorScheme();
  const [releaseNotesOpened, setReleaseNotesOpened] = useState(false);
  const releaseNotesRequest = useQuery({
    enabled: false,
    queryKey: ["release-notes"],
    queryFn: releaseNotesLoader,
    retry: false,
    staleTime: Number.POSITIVE_INFINITY,
  });
  const releaseNotesStatus: ReleaseNotesStatus = releaseNotesRequest.isFetching
    ? "loading"
    : releaseNotesRequest.isError
      ? "error"
      : releaseNotesRequest.data === undefined
        ? "idle"
        : "ready";
  const releaseNotesLanguage = resolveReleaseNotesLocale(i18n.resolvedLanguage);

  /** 打开弹窗并仅在尚无成功缓存时请求候选资源。 */
  const requestReleaseNotes = (): void => {
    setReleaseNotesOpened(true);
    if (releaseNotesRequest.data === undefined) void releaseNotesRequest.refetch();
  };

  const languageUpdate = useMutation({
    mutationFn: async (nextLanguage: InterfaceLanguage) => {
      const authoritative = await syncShellInterfaceLanguage(nextLanguage);
      await i18n.changeLanguage(authoritative);
      return authoritative;
    },
    onSuccess: (authoritative) => {
      persistInterfaceLanguage(authoritative);
      setLanguage(authoritative);
    },
  });

  return (
    <Stack data-testid="settings-page" gap="lg">
      <Stack gap={4}>
        <Title order={1}>{t("settings.title")}</Title>
        <Text c="dimmed">{t("settings.description")}</Text>
      </Stack>

      <Paper
        className="surface-card"
        data-testid="settings-release-notes-section"
        p="xl"
        radius="lg"
        withBorder
      >
        <Stack gap="md">
          <Group justify="space-between" wrap="wrap">
            <Title order={2} size="h3">
              {t("settings.application_title")}
            </Title>
            {metadata.data === undefined ? (
              <Skeleton h={28} radius="xl" w={92} />
            ) : (
              <Badge size="lg" variant="light">
                {t("settings.version")} {formatDisplayVersion(metadata.data.version)}
              </Badge>
            )}
          </Group>
          <Stack gap={2}>
            <Text fw={650} size="lg">
              {metadata.data?.applicationName ?? t("identity.application_name")}
            </Text>
            <Text c="dimmed" size="sm">
              {t("settings.localized_name")}: {t("identity.localized_name")}
            </Text>
          </Stack>
          <Text c="dimmed" size="sm">
            {t("settings.application_description")}
          </Text>
        </Stack>
      </Paper>

      <SimpleGrid cols={{ base: 1, lg: 2 }} spacing="lg">
        <Paper className="surface-card" p="xl" radius="lg" withBorder>
          <Stack gap="md">
            <Title order={2} size="h3">
              {t("settings.language_title")}
            </Title>
            <Text c="dimmed" size="sm">
              {t("settings.language_description")}
            </Text>
            <SegmentedControl
              aria-label={t("settings.language_title")}
              data={[
                { label: t("settings.language_zh_cn"), value: "zh-CN" },
                { label: t("settings.language_en_us"), value: "en-US" },
              ]}
              disabled={languageUpdate.isPending}
              onChange={(value) => {
                if (isInterfaceLanguage(value) && value !== language) {
                  languageUpdate.mutate(value);
                }
              }}
              value={language}
            />
            {languageUpdate.isError ? (
              <Alert
                icon={<IconAlertCircle aria-hidden="true" size={18} />}
                role="alert"
                title={t("settings.language_error_title")}
              >
                {t("settings.language_error")}
              </Alert>
            ) : null}
          </Stack>
        </Paper>

        <Paper className="surface-card" p="xl" radius="lg" withBorder>
          <Stack gap="md">
            <Title order={2} size="h3">
              {t("settings.theme_title")}
            </Title>
            <Text c="dimmed" size="sm">
              {t("settings.theme_description")}
            </Text>
            <SegmentedControl
              aria-label={t("settings.theme_title")}
              data={[
                { label: t("settings.theme_light"), value: "light" },
                { label: t("settings.theme_dark"), value: "dark" },
                { label: t("settings.theme_system"), value: "auto" },
              ]}
              onChange={(value) => {
                if (isAppColorScheme(value)) setColorScheme(value);
              }}
              value={colorScheme}
            />
          </Stack>
        </Paper>
      </SimpleGrid>

      <SimpleGrid cols={{ base: 1, lg: 2 }} spacing="lg">
        <HostCapabilitySwitch
          getEnabled={getSystemNotificationSetting}
          id="system_notification"
          queryKey={systemNotificationQuery.queryKey}
          setEnabled={setSystemNotificationEnabled}
        />
        <HostCapabilitySwitch
          getEnabled={getAutostartEnabled}
          id="autostart"
          queryKey={autostartQuery.queryKey}
          setEnabled={setAutostartEnabled}
        />
      </SimpleGrid>

      <Paper className="surface-card" p="xl" radius="lg" withBorder>
        <Group align="center" justify="space-between" wrap="wrap">
          <Stack gap={4} maw={620}>
            <Title order={2} size="h3">
              {t("settings.release_notes_title")}
            </Title>
            <Text c="dimmed" size="sm">
              {t("settings.release_notes_description")}
            </Text>
          </Stack>
          <Button
            leftSection={<IconHistory aria-hidden="true" size={18} />}
            onClick={requestReleaseNotes}
            variant="light"
          >
            {t("settings.release_notes_action")}
          </Button>
        </Group>
      </Paper>

      <ReleaseNotesDialogTemplate
        language={releaseNotesLanguage}
        onClose={() => {
          setReleaseNotesOpened(false);
        }}
        onRetry={() => {
          void releaseNotesRequest.refetch();
        }}
        opened={releaseNotesOpened}
        releases={releaseNotesRequest.data}
        status={releaseNotesStatus}
      />
    </Stack>
  );
}
