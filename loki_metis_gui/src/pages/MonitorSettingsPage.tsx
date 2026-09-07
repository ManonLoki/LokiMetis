import {
  Alert,
  Button,
  Card,
  Code,
  CopyButton,
  Group,
  Stack,
  Tabs,
  Text,
  TextInput,
  Title,
} from "@mantine/core";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";

import {
  selectAvailableMonitorAiTools,
  selectEnabledAvailableMonitorTools,
} from "../ai-capabilities";
import {
  chooseMonitorHookDirectory,
  getMonitorCapabilities,
  getMonitorSettings,
  listMonitorHookLocations,
  saveMonitorHookDirectory,
  writeMonitorHookConfig,
  type HookConfigLocation,
  type MonitorAiTool,
  type MonitorHookWriteOutcome,
} from "../api/monitor";
import { visibleErrorMessage } from "../visible-error";

const activationCommands: Partial<Record<MonitorHookWriteOutcome, readonly string[]>> = {
  hermesEnableRequired: ["hermes plugins enable lokimetis"],
  openClawEnableRequired: [
    "openclaw plugins enable lokimetis",
    "openclaw config set plugins.entries.lokimetis.hooks.allowConversationAccess true",
    "openclaw gateway restart",
  ],
};

/** 仅为需要额外激活的插件展示紧凑、可复制的命令。 */
function ActivationGuidance({ outcome }: { outcome: MonitorHookWriteOutcome }) {
  const { t } = useTranslation();
  const commands = activationCommands[outcome];
  if (!commands) return null;

  return (
    <Alert color="yellow" data-testid="hook-activation-guidance" variant="light">
      <Stack gap={6}>
        <Text size="xs">{t(`monitor.settings.activation.${outcome}`)}</Text>
        {commands.map((command) => (
          <Group gap="xs" key={command} wrap="nowrap">
            <Code block style={{ flex: 1, minWidth: 0, overflowX: "auto" }}>
              {command}
            </Code>
            <CopyButton timeout={1500} value={command}>
              {({ copied, copy }) => (
                <Button
                  aria-label={t("monitor.settings.activation.copyCommandAria", {
                    command,
                  })}
                  color={copied ? "teal" : "gray"}
                  onClick={copy}
                  size="xs"
                  variant="subtle"
                >
                  {t(
                    copied
                      ? "monitor.settings.activation.copied"
                      : "monitor.settings.activation.copy",
                  )}
                </Button>
              )}
            </CopyButton>
          </Group>
        ))}
      </Stack>
    </Alert>
  );
}

/** 公共设置页中的 Hooks 配置：消费统一 Agent 选择并管理已启用工具。 */
export function MonitorSettingsPage() {
  const { t } = useTranslation();
  const queryClient = useQueryClient();
  const capabilities = useQuery({
    queryFn: getMonitorCapabilities,
    queryKey: ["monitor-capabilities"],
  });
  const settings = useQuery({
    queryFn: getMonitorSettings,
    queryKey: ["monitor-settings"],
  });
  const locations = useQuery({
    queryFn: listMonitorHookLocations,
    queryKey: ["monitor-hook-locations"],
  });
  const write = useMutation({
    mutationFn: (tool: MonitorAiTool) => writeMonitorHookConfig(tool),
    onSuccess: () => {
      void queryClient.invalidateQueries({ queryKey: ["monitor-hook-locations"] });
    },
  });
  const directoryDraftsInitialized = useRef(false);
  const [directoryDrafts, setDirectoryDrafts] = useState<
    Partial<Record<MonitorAiTool, string>>
  >({});
  const [selectingDirectory, setSelectingDirectory] = useState<MonitorAiTool | null>(null);
  const [pickerError, setPickerError] = useState<unknown>(null);
  useEffect(() => {
    if (!locations.data || directoryDraftsInitialized.current) return;
    setDirectoryDrafts(
      Object.fromEntries(locations.data.map(({ tool, directory }) => [tool, directory])),
    );
    directoryDraftsInitialized.current = true;
  }, [locations.data]);
  const saveDirectory = useMutation({
    mutationFn: ({ tool, directory }: { tool: MonitorAiTool; directory: string }) =>
      saveMonitorHookDirectory(tool, directory),
    onSuccess: (saved) => {
      queryClient.setQueryData<HookConfigLocation[]>(
        ["monitor-hook-locations"],
        (current = []) => current.map((item) => (item.tool === saved.tool ? saved : item)),
      );
      setDirectoryDrafts((current) => ({
        ...current,
        [saved.tool]: saved.directory,
      }));
      write.reset();
    },
  });
  const tools = selectAvailableMonitorAiTools(capabilities.data?.aiTools ?? []);
  const enabled = selectEnabledAvailableMonitorTools(
    settings.data?.enabledAiTools ?? [],
    tools,
  );
  const visibleTools = tools.filter((item) => enabled.includes(item.tool));
  const [selectedTool, setSelectedTool] = useState<MonitorAiTool | null>(null);
  const activeTool = visibleTools.some((item) => item.tool === selectedTool)
    ? selectedTool
    : (visibleTools[0]?.tool ?? null);
  return (
    <Stack className="settings-page" data-testid="monitor-settings" gap="sm">
      <Card
        aria-labelledby="monitor-hooks-management-title"
        className="surface-card settings-card"
        data-testid="monitor-hooks-management"
        p="sm"
        radius="lg"
        role="region"
        withBorder
      >
        <Stack gap="sm">
          <div>
            <Title id="monitor-hooks-management-title" order={3}>
              {t("monitor.settings.title")}
            </Title>
          </div>
          {settings.error ||
          capabilities.error ||
          locations.error ||
          saveDirectory.error ||
          pickerError ? (
            <Alert color="red">
              {visibleErrorMessage(
                settings.error ??
                  capabilities.error ??
                  locations.error ??
                  saveDirectory.error ??
                  pickerError,
              )}
            </Alert>
          ) : null}
          {visibleTools.length === 0 ? (
            <Alert color="blue" variant="light">
              {t("monitor.settings.chooseAgentFirst")}
            </Alert>
          ) : (
            <Tabs
              className="ai-tool-tabs ai-tool-tabs-compact"
              keepMounted={false}
              onChange={(value) => {
                if (value) {
                  setSelectedTool(value as MonitorAiTool);
                  setPickerError(null);
                  saveDirectory.reset();
                  write.reset();
                }
              }}
              value={activeTool}
            >
              <Tabs.List grow>
                {visibleTools.map((item) => (
                  <Tabs.Tab key={item.tool} value={item.tool}>
                    {item.name}
                  </Tabs.Tab>
                ))}
              </Tabs.List>
              {visibleTools.map((item) => {
                const location = locations.data?.find((entry) => entry.tool === item.tool);
                const directoryDraft =
                  directoryDrafts[item.tool] ?? location?.directory ?? "";
                const pathDirty =
                  Boolean(location) && directoryDraft.trim() !== location?.directory;
                const isCurrentWrite = write.variables === item.tool;
                return (
                  <Tabs.Panel key={item.tool} pt="xs" value={item.tool}>
                    <Stack gap="sm">
                      <TextInput
                        label={t("monitor.settings.directory")}
                        onChange={(event) => {
                          const directory = event.currentTarget.value;
                          setDirectoryDrafts((current) => ({
                            ...current,
                            [item.tool]: directory,
                          }));
                          setPickerError(null);
                          saveDirectory.reset();
                          write.reset();
                        }}
                        placeholder={t("monitor.settings.directoryPlaceholder")}
                        size="xs"
                        value={directoryDraft}
                      />
                      <TextInput
                        label={t("monitor.settings.configFile")}
                        readOnly
                        size="xs"
                        value={location?.configPath ?? ""}
                      />
                      <Group justify="space-between" wrap="wrap">
                        <Group gap="xs" wrap="wrap">
                          <Button
                            loading={selectingDirectory === item.tool}
                            onClick={async () => {
                              setSelectingDirectory(item.tool);
                              setPickerError(null);
                              try {
                                const selected = await chooseMonitorHookDirectory(
                                  directoryDraft || location?.directory || "",
                                  t("monitor.settings.directoryDialogTitle"),
                                );
                                if (selected) {
                                  setDirectoryDrafts((current) => ({
                                    ...current,
                                    [item.tool]: selected,
                                  }));
                                  saveDirectory.reset();
                                  write.reset();
                                }
                              } catch (error) {
                                setPickerError(error);
                              } finally {
                                setSelectingDirectory(null);
                              }
                            }}
                            size="xs"
                            variant="default"
                          >
                            {t("monitor.settings.chooseDirectory")}
                          </Button>
                          <Button
                            disabled={!directoryDraft.trim() || !pathDirty}
                            loading={
                              saveDirectory.isPending &&
                              saveDirectory.variables?.tool === item.tool &&
                              saveDirectory.variables.directory !== ""
                            }
                            onClick={() =>
                              saveDirectory.mutate({
                                tool: item.tool,
                                directory: directoryDraft,
                              })
                            }
                            size="xs"
                            variant="default"
                          >
                            {t("monitor.settings.saveDirectory")}
                          </Button>
                          <Button
                            disabled={!location?.isCustom}
                            loading={
                              saveDirectory.isPending &&
                              saveDirectory.variables?.tool === item.tool &&
                              saveDirectory.variables.directory === ""
                            }
                            onClick={() =>
                              saveDirectory.mutate({ tool: item.tool, directory: "" })
                            }
                            size="xs"
                            variant="subtle"
                          >
                            {t("monitor.settings.restoreDirectory")}
                          </Button>
                        </Group>
                        <Button
                          disabled={pathDirty || locations.isPending}
                          loading={write.isPending && isCurrentWrite}
                          onClick={() => write.mutate(item.tool)}
                          size="xs"
                        >
                          {t("monitor.settings.write")}
                        </Button>
                      </Group>
                      {write.error && isCurrentWrite ? (
                        <Alert color="red">{visibleErrorMessage(write.error)}</Alert>
                      ) : null}
                      {write.data?.tool === item.tool ? (
                        <>
                          <Alert aria-live="polite" color="green">
                            {t("monitor.settings.written", {
                              file: write.data.filename,
                              outcome: t(`monitor.outcome.${write.data.outcome}`),
                            })}
                          </Alert>
                          <ActivationGuidance outcome={write.data.outcome} />
                        </>
                      ) : null}
                    </Stack>
                  </Tabs.Panel>
                );
              })}
            </Tabs>
          )}
        </Stack>
      </Card>
    </Stack>
  );
}
