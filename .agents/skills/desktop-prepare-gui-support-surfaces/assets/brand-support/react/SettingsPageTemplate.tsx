import {
  Alert,
  Badge,
  Button,
  Group,
  Paper,
  SegmentedControl,
  Stack,
  Switch,
  Text,
  Title,
  useMantineColorScheme,
} from "@mantine/core";
import { IconHistory } from "@tabler/icons-react";
import {
  type ReactElement,
  useCallback,
  useEffect,
  useRef,
  useState,
} from "react";
import { useTranslation } from "react-i18next";

import type { AppColorScheme } from "./AppThemeProviderTemplate";
import { formatDisplayVersion } from "./displayVersion";
import type { LocalizedReleaseNoteEntry } from "./releaseNotes";
import { loadBundledReleaseNotes } from "./releaseNotesResource";
import { ReleaseNotesDialogTemplate } from "./ReleaseNotesDialogTemplate";

/** 设置页固定支持的界面语言。 */
export type SupportedInterfaceLanguage = "zh-CN" | "en-US";

/** 由设置路由传入的异步布尔宿主能力。 */
export interface AsyncHostCapabilitySetting {
  enabled: boolean;
  getEnabled: () => Promise<boolean>;
  onChange: (enabled: boolean) => Promise<boolean>;
}

/** 固定设置页所需的应用事实和本地展示偏好。 */
export interface SettingsPageTemplateProps {
  applicationName: string;
  version: string;
  language: SupportedInterfaceLanguage;
  onLanguageChange: (language: SupportedInterfaceLanguage) => void;
  autostart?: AsyncHostCapabilitySetting;
  releaseNotesLoader?: () => Promise<readonly LocalizedReleaseNoteEntry[]>;
  systemNotification?: AsyncHostCapabilitySetting;
}

type CapabilityId = "autostart" | "system_notification";

interface CapabilitySwitchProps {
  capability: AsyncHostCapabilitySetting;
  id: CapabilityId;
}

type CapabilitySwitchValue = boolean | "unknown";

/** 提交宿主能力切换，并始终以命令返回或重新读取的权威状态同步 UI。 */
function CapabilitySwitch({
  capability,
  id,
}: CapabilitySwitchProps): ReactElement {
  const { t } = useTranslation("brandSupport");
  const titleId = `settings-capability-title-${id}`;
  const descriptionId = `settings-capability-description-${id}`;
  const title = t(`settings.${id}_title`);
  const description = t(`settings.${id}_description`);
  const [checked, setChecked] = useState<CapabilitySwitchValue>(
    capability.enabled,
  );
  const [status, setStatus] = useState<
    "idle" | "pending" | "enabled" | "disabled" | "error" | "unknown"
  >("idle");

  useEffect(() => {
    setChecked(capability.enabled);
    setStatus("idle");
  }, [capability.enabled]);

  const rereadAuthoritativeState = async (): Promise<boolean | null> => {
    try {
      const authoritativeEnabled = await capability.getEnabled();
      setChecked(authoritativeEnabled);
      return authoritativeEnabled;
    } catch {
      setChecked("unknown");
      return null;
    }
  };

  const retryReadAuthoritativeState = async (): Promise<void> => {
    setStatus("pending");
    const authoritativeEnabled = await rereadAuthoritativeState();
    setStatus(
      authoritativeEnabled === null
        ? "unknown"
        : authoritativeEnabled
          ? "enabled"
          : "disabled",
    );
  };

  const updateSetting = async (nextEnabled: boolean): Promise<void> => {
    setStatus("pending");
    try {
      const authoritativeEnabled = await capability.onChange(nextEnabled);
      setChecked(authoritativeEnabled);
      setStatus(authoritativeEnabled ? "enabled" : "disabled");
    } catch {
      const authoritativeEnabled = await rereadAuthoritativeState();
      setStatus(authoritativeEnabled === null ? "unknown" : "error");
    }
  };

  return (
    <Paper
      data-testid={`settings-capability-${id}`}
      p="lg"
      radius="lg"
      withBorder
    >
      <Stack gap="sm">
        <Group align="flex-start" justify="space-between" wrap="nowrap">
          <Stack gap={4} style={{ flex: 1 }}>
            <Text data-testid={titleId} fw={500} id={titleId}>
              {title}
            </Text>
            <Text
              c="dimmed"
              data-testid={descriptionId}
              id={descriptionId}
              size="sm"
            >
              {description}
            </Text>
          </Stack>
          <Switch
            aria-describedby={descriptionId}
            aria-labelledby={titleId}
            checked={checked === true}
            disabled={status === "pending" || status === "unknown"}
            data-authoritative-state={
              checked === "unknown"
                ? "unknown"
                : checked
                  ? "enabled"
                  : "disabled"
            }
            onChange={(event) => {
              void updateSetting(event.currentTarget.checked);
            }}
          />
        </Group>
        {status !== "idle" && status !== "error" ? (
          <Text aria-live="polite" role="status" size="sm">
            {t(`settings.capability_${status}`)}
          </Text>
        ) : null}
        {status === "error" ? (
          <Alert role="alert" title={t("settings.capability_error_title")}>
            {t(`settings.${id}_error`)}
          </Alert>
        ) : null}
        {status === "unknown" ? (
          <>
            <Alert role="alert" title={t("settings.capability_unknown_title")}>
              {t(`settings.${id}_unknown`)}
            </Alert>
            <Button
              onClick={() => {
                void retryReadAuthoritativeState();
              }}
              variant="light"
            >
              {t("settings.capability_retry")}
            </Button>
          </>
        ) : null}
      </Stack>
    </Paper>
  );
}

/** 把 SegmentedControl 字符串收敛为固定语言枚举。 */
function isSupportedLanguage(
  value: string,
): value is SupportedInterfaceLanguage {
  return value === "zh-CN" || value === "en-US";
}

/** 把 SegmentedControl 字符串收敛为固定主题偏好枚举。 */
function isSupportedColorScheme(value: string): value is AppColorScheme {
  return value === "light" || value === "dark" || value === "auto";
}

/** 渲染固定设置页，并按初始化选择加入 Rust-only 宿主能力开关。 */
export function SettingsPageTemplate({
  applicationName,
  version,
  language,
  onLanguageChange,
  autostart,
  releaseNotesLoader = loadBundledReleaseNotes,
  systemNotification,
}: SettingsPageTemplateProps): ReactElement {
  const { t } = useTranslation("brandSupport");
  const { colorScheme, setColorScheme } = useMantineColorScheme();
  const [releaseNotesOpened, setReleaseNotesOpened] = useState(false);
  const [releaseNotes, setReleaseNotes] = useState<
    readonly LocalizedReleaseNoteEntry[]
  >([]);
  const [releaseNotesStatus, setReleaseNotesStatus] = useState<
    "idle" | "loading" | "ready" | "error"
  >("idle");
  const releaseNotesRequest = useRef(0);

  useEffect(
    () => () => {
      releaseNotesRequest.current += 1;
    },
    [],
  );

  /** 只通过已注册的窄命令加载候选资源，并忽略卸载后的异步结果。 */
  const requestReleaseNotes = useCallback(() => {
    const request = releaseNotesRequest.current + 1;
    releaseNotesRequest.current = request;
    setReleaseNotesStatus("loading");
    void releaseNotesLoader()
      .then((loaded) => {
        if (releaseNotesRequest.current !== request) return;
        setReleaseNotes(loaded);
        setReleaseNotesStatus("ready");
      })
      .catch(() => {
        if (releaseNotesRequest.current !== request) return;
        setReleaseNotes([]);
        setReleaseNotesStatus("error");
      });
  }, [releaseNotesLoader]);

  /** 打开弹窗时首次加载资源；已成功加载的同一候选内容在本页复用。 */
  const openReleaseNotes = useCallback(() => {
    setReleaseNotesOpened(true);
    if (releaseNotesStatus === "idle") requestReleaseNotes();
  }, [releaseNotesStatus, requestReleaseNotes]);

  return (
    <Stack data-testid="settings-page" gap="lg">
      <Title order={2}>{t("settings.title")}</Title>

      <Paper
        data-testid="settings-application-section"
        p="lg"
        radius="lg"
        withBorder
      >
        <Stack gap="sm">
          <Group justify="space-between" wrap="wrap">
            <Title order={3}>{t("settings.application_title")}</Title>
            <Badge size="lg" variant="light">
              {t("settings.version")} {formatDisplayVersion(version)}
            </Badge>
          </Group>
          <Text fw={500}>{applicationName}</Text>
          <Text c="dimmed" size="sm">
            {t("settings.application_description")}
          </Text>
        </Stack>
      </Paper>

      <Paper p="lg" radius="lg" withBorder>
        <Stack gap="md">
          <Title order={3}>{t("settings.language_title")}</Title>
          <Text c="dimmed" size="sm">
            {t("settings.language_description")}
          </Text>
          <SegmentedControl
            aria-label={t("settings.language_title")}
            data={[
              { label: t("settings.language_zh_cn"), value: "zh-CN" },
              { label: t("settings.language_en_us"), value: "en-US" },
            ]}
            onChange={(value) => {
              if (isSupportedLanguage(value)) {
                onLanguageChange(value);
              }
            }}
            value={language}
          />
        </Stack>
      </Paper>

      <Paper p="lg" radius="lg" withBorder>
        <Stack gap="md">
          <Title order={3}>{t("settings.theme_title")}</Title>
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
              if (isSupportedColorScheme(value)) {
                setColorScheme(value);
              }
            }}
            value={colorScheme}
          />
        </Stack>
      </Paper>

      <Paper
        data-testid="settings-release-notes-section"
        p="lg"
        radius="lg"
        withBorder
      >
        <Stack gap="md">
          <Group justify="space-between" wrap="wrap">
            <Stack gap={4}>
              <Title order={3}>{t("settings.release_notes_title")}</Title>
              <Text c="dimmed" size="sm">
                {t("settings.release_notes_description")}
              </Text>
            </Stack>
            <Group data-testid="settings-release-notes-actions" gap="xs">
              <Button
                leftSection={<IconHistory aria-hidden="true" size={18} />}
                onClick={openReleaseNotes}
                variant="default"
              >
                {t("settings.release_notes_action")}
              </Button>
            </Group>
          </Group>
        </Stack>
      </Paper>

      <ReleaseNotesDialogTemplate
        onClose={() => setReleaseNotesOpened(false)}
        onRetry={requestReleaseNotes}
        opened={releaseNotesOpened}
        releases={releaseNotes}
        status={releaseNotesStatus}
      />

      {systemNotification ? (
        <CapabilitySwitch
          capability={systemNotification}
          id="system_notification"
        />
      ) : null}

      {autostart ? (
        <CapabilitySwitch capability={autostart} id="autostart" />
      ) : null}
    </Stack>
  );
}
