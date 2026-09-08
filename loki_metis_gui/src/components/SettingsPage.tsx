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
import { IconAlertCircle, IconBrandGithub, IconHistory } from "@tabler/icons-react";
import { openUrl } from "@tauri-apps/plugin-opener";
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
import { AgentSettings } from "./AgentSettings";
import type { AppColorScheme } from "./AppThemeProvider";
import { syncShellInterfaceLanguage } from "./AppShell";
import { HostCapabilitySwitch } from "./HostCapabilitySwitch";
import { ReleaseNotesDialogTemplate, type ReleaseNotesStatus } from "./ReleaseNotesDialog";
import { SponsorPaymentPanel } from "./SponsorPaymentPanel";

/** 应用信息面板唯一允许交给系统浏览器打开的外部地址。 */
const GITHUB_REPOSITORY_URL = "https://github.com/ManonLoki/LokiMetis";

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

/** 渲染应用信息、Agent 配置、本地偏好、更新日志及已启用宿主能力。 */
export function SettingsPage({
  releaseNotesLoader = loadBundledReleaseNotes,
}: SettingsPageProps = {}): ReactElement {
  const { i18n, t } = useTranslation();
  const metadata = useQuery(appMetadataQuery);
  const [language, setLanguage] = useAtom(interfaceLanguageAtom);
  const { colorScheme, setColorScheme } = useMantineColorScheme();
  const [releaseNotesOpened, setReleaseNotesOpened] = useState(false);
  const [repositoryOpenFailed, setRepositoryOpenFailed] = useState(false);
  const releaseNotesRequest = useQuery({
    enabled: false,
    queryKey: ["release-notes"],
    queryFn: () => releaseNotesLoader(),
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

  /** 仅把已批准的固定仓库地址交给系统默认浏览器，并在失败时留在当前页。 */
  const openGitHubRepository = async (): Promise<void> => {
    setRepositoryOpenFailed(false);
    try {
      await openUrl(GITHUB_REPOSITORY_URL);
    } catch {
      setRepositoryOpenFailed(true);
    }
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
      <Paper
        className="surface-card"
        data-testid="settings-application-section"
        p="xl"
        radius="lg"
        withBorder
      >
        <Group align="flex-start" justify="space-between" wrap="wrap">
          <Stack gap="md" maw={620}>
            <Title order={2} size="h3">
              {t("settings.application_title")}
            </Title>
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
          <Stack align="flex-end" gap="md">
            {metadata.data === undefined ? (
              <Skeleton h={28} radius="xl" w={92} />
            ) : (
              <Badge size="lg" variant="light">
                {t("settings.version")} {formatDisplayVersion(metadata.data.version)}
              </Badge>
            )}
            <Button
              leftSection={<IconHistory aria-hidden="true" size={18} />}
              onClick={requestReleaseNotes}
              variant="light"
            >
              {t("settings.release_notes_action")}
            </Button>
            <Button
              leftSection={<IconBrandGithub aria-hidden="true" size={18} />}
              onClick={() => void openGitHubRepository()}
              variant="subtle"
            >
              {t("settings.github_repository_action")}
            </Button>
          </Stack>
        </Group>
        {repositoryOpenFailed ? (
          <Alert
            icon={<IconAlertCircle aria-hidden="true" size={18} />}
            mt="lg"
            role="alert"
            title={t("settings.github_repository_error_title")}
          >
            {t("settings.github_repository_error")}
          </Alert>
        ) : null}
      </Paper>

      <AgentSettings />

      <SimpleGrid cols={{ base: 1, lg: 2 }} spacing="lg">
        <Paper className="surface-card" p="xl" radius="lg" withBorder>
          <Stack gap="md">
            <Title order={2} size="h3">
              {t("settings.language_title")}
            </Title>
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

      <SponsorPaymentPanel />

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
