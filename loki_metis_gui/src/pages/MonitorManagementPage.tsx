import {
  Alert,
  Badge,
  Button,
  Card,
  Group,
  Loader,
  SimpleGrid,
  Stack,
  Tabs,
  Text,
  Textarea,
} from "@mantine/core";
import { IconCheck } from "@tabler/icons-react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";

import {
  getMonitorCapabilities,
  getMonitorSettings,
  imageUploadAcceptValue,
  listMonitorImages,
  listMonitorProfileDrafts,
  saveMonitorImage,
  fileBytes,
  saveMonitorProfileDraft,
  type MonitorAiTool,
  type MonitorHookBehavior,
  type MonitorImageGallery,
  type MonitorImagePreview,
  type MonitorProfileDraft,
} from "../api/monitor";
import { ImagePicker } from "../components/monitor/ImagePicker";
import { SlotPicker } from "../components/monitor/SlotPicker";
import { visibleErrorMessage } from "../visible-error";

/** 行为卡片状态色，仅用于展示。 */
const behaviorColors: Record<MonitorHookBehavior, string> = {
  idle: "gray",
  running: "violet",
  asking: "yellow",
  error: "red",
};

/** 本机保存按追加写入，图库末项即本次新图。 */
function newlySavedMonitorImage(
  gallery: MonitorImageGallery,
): MonitorImagePreview | undefined {
  return gallery.images.at(-1);
}

/** 监控管理：按已启用 Agent 配置展示位与行为图片，图片来自本机图库。 */
export function MonitorManagementPage() {
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
  const profiles = useQuery({
    queryFn: listMonitorProfileDrafts,
    queryKey: ["monitor-profile-drafts"],
  });
  const images = useQuery({ queryFn: listMonitorImages, queryKey: ["monitor-images"] });
  const enabledTools = settings.data?.enabledAiTools ?? [];
  const visibleTools = (capabilities.data?.aiTools ?? []).filter((item) =>
    enabledTools.includes(item.tool),
  );
  const [selectedTool, setSelectedTool] = useState<MonitorAiTool | null>(null);
  const [drafts, setDrafts] = useState<Partial<Record<MonitorAiTool, MonitorProfileDraft>>>(
    {},
  );
  const activeTool = visibleTools.some((item) => item.tool === selectedTool)
    ? selectedTool
    : (visibleTools[0]?.tool ?? null);
  useEffect(() => {
    if (!profiles.data) return;
    setDrafts(
      Object.fromEntries(
        profiles.data.drafts.map((profile) => [profile.tool, profile]),
      ) as Partial<Record<MonitorAiTool, MonitorProfileDraft>>,
    );
  }, [profiles.data]);
  const draft = activeTool ? drafts[activeTool] : undefined;
  const availableImageIds = new Set((images.data?.images ?? []).map((image) => image.id));
  const configuredBehaviorCount = (draft?.hooks ?? []).filter(
    (hook) => hook.image.length > 0 && availableImageIds.has(hook.image),
  ).length;
  const isComplete = draft !== undefined && configuredBehaviorCount === draft.hooks.length;
  const save = useMutation({
    mutationFn: saveMonitorProfileDraft,
    onSuccess: (next) => {
      setDrafts((current) => ({ ...current, [next.tool]: next }));
      void queryClient.invalidateQueries({ queryKey: ["monitor-profile-drafts"] });
    },
  });
  const [uploadedName, setUploadedName] = useState<string | null>(null);
  const upload = useMutation({
    mutationFn: async ({
      file,
      tool,
      behavior,
    }: {
      file: File;
      tool: MonitorAiTool;
      behavior: MonitorHookBehavior;
    }) => {
      const gallery = await saveMonitorImage(file.name, await fileBytes(file));
      return { gallery, filename: file.name, tool, behavior };
    },
    onSuccess: ({ gallery, filename, tool, behavior }) => {
      queryClient.setQueryData(["monitor-images"], gallery);
      const uploaded = newlySavedMonitorImage(gallery);
      if (!uploaded) return;
      setUploadedName(filename);
      setDrafts((current) => {
        const currentDraft = current[tool];
        if (!currentDraft) return current;
        return {
          ...current,
          [tool]: {
            ...currentDraft,
            hooks: currentDraft.hooks.map((hook) =>
              hook.behavior === behavior ? { ...hook, image: uploaded.id } : hook,
            ),
          },
        };
      });
    },
  });
  const updateDraft = (next: MonitorProfileDraft) => {
    save.reset();
    setDrafts((current) => ({ ...current, [next.tool]: next }));
  };
  const updateHookField = (
    behaviorValue: MonitorHookBehavior,
    field: "image" | "content",
    value: string,
  ) => {
    if (!draft) return;
    updateDraft({
      ...draft,
      hooks: draft.hooks.map((item) =>
        item.behavior === behaviorValue ? { ...item, [field]: value } : item,
      ),
    });
  };
  const blockingError =
    profiles.error ?? capabilities.error ?? settings.error ?? images.error;
  const mutationError = upload.error ?? save.error;
  const availableImages = images.data?.images ?? [];
  const uploadAccept = imageUploadAcceptValue(capabilities.data?.imageUploadAccept);
  if (blockingError) return <Alert color="red">{visibleErrorMessage(blockingError)}</Alert>;
  const catalogPending = settings.isPending || capabilities.isPending || profiles.isPending;
  if (catalogPending) {
    return (
      <Stack align="center" data-testid="monitor-management" py="xl">
        <Loader size="sm" />
      </Stack>
    );
  }
  if (visibleTools.length === 0) {
    return (
      <Alert color="blue" data-testid="monitor-management">
        {t("monitor.management.noClient")}
      </Alert>
    );
  }
  if (!activeTool || !draft || !capabilities.data) {
    return (
      <Stack align="center" data-testid="monitor-management" py="xl">
        <Loader size="sm" />
      </Stack>
    );
  }
  return (
    <Stack data-testid="monitor-management" gap="md">
      {mutationError ? (
        <Alert color="red">{visibleErrorMessage(mutationError)}</Alert>
      ) : null}
      {uploadedName ? (
        <Alert color="teal" variant="light">
          {t("monitor.picker.uploadedAndSelected", { filename: uploadedName })}
        </Alert>
      ) : null}
      <Tabs
        className="ai-tool-tabs"
        keepMounted={false}
        onChange={(value) => {
          if (!value) return;
          save.reset();
          setSelectedTool(value as MonitorAiTool);
        }}
        value={activeTool}
      >
        <Tabs.List grow>
          {visibleTools.map((tool) => (
            <Tabs.Tab key={tool.tool} value={tool.tool}>
              {tool.name}
            </Tabs.Tab>
          ))}
        </Tabs.List>
        {visibleTools.map((tool) => (
          <Tabs.Panel key={tool.tool} pt="md" value={tool.tool}>
            <Stack gap="lg">
              <Card className="surface-card slot-section-card" p="lg" withBorder>
                <SlotPicker
                  max={capabilities.data.profileSlot.max}
                  min={capabilities.data.profileSlot.min}
                  onChange={(slot) => updateDraft({ ...draft, slot })}
                  value={draft.slot}
                />
              </Card>
              <Group align="flex-end" justify="space-between">
                <div>
                  <Text fw={650}>{t("monitor.management.behaviorDisplay")}</Text>
                  <Text c="dimmed" mt={3} size="sm">
                    {t("monitor.management.behaviorDescription")}
                  </Text>
                </div>
                <Badge color="violet" size="lg" variant="light">
                  {t("monitor.management.configuredCount", {
                    configured: configuredBehaviorCount,
                    total: draft.hooks.length,
                  })}
                </Badge>
              </Group>
              {!images.isPending && availableImages.length === 0 ? (
                <Alert color="yellow" variant="light">
                  {t("monitor.management.noImages")}
                </Alert>
              ) : null}
              <SimpleGrid cols={{ base: 1, sm: 2 }} spacing="lg">
                {draft.hooks.map((hook) => {
                  const behaviorValue = hook.behavior;
                  return (
                    <Card
                      className="behavior-card"
                      data-behavior={behaviorValue}
                      key={behaviorValue}
                      p={0}
                      withBorder
                    >
                      <div className="behavior-card-header">
                        <Group justify="space-between" wrap="nowrap">
                          <Group gap="sm" wrap="nowrap">
                            <span className="behavior-card-status" />
                            <div>
                              <Text fw={700}>{t(`monitor.behavior.${behaviorValue}`)}</Text>
                              <Text c="dimmed" mt={1} size="xs">
                                {t("monitor.management.stateDescription")}
                              </Text>
                            </div>
                          </Group>
                          <Badge
                            color={hook.image ? "teal" : behaviorColors[behaviorValue]}
                            radius="sm"
                            variant="light"
                          >
                            {hook.image
                              ? t("monitor.management.configured")
                              : t("monitor.management.notConfigured")}
                          </Badge>
                        </Group>
                      </div>
                      <Stack className="behavior-card-content" gap="md">
                        <ImagePicker
                          counts={images.data?.counts}
                          disabled={images.isPending || upload.isPending}
                          images={availableImages}
                          onChange={(value) => {
                            setUploadedName(null);
                            updateHookField(behaviorValue, "image", value);
                          }}
                          onUpload={(file) => {
                            upload.mutate({
                              file,
                              tool: tool.tool,
                              behavior: behaviorValue,
                            });
                          }}
                          uploadAccept={uploadAccept}
                          uploading={
                            upload.isPending &&
                            upload.variables?.tool === tool.tool &&
                            upload.variables?.behavior === behaviorValue
                          }
                          value={hook.image}
                        />
                        <Textarea
                          label={
                            <span>
                              {t("monitor.management.content")}{" "}
                              <Text c="dimmed" component="span" fw={400} size="xs">
                                {t("monitor.management.optional")}
                              </Text>
                            </span>
                          }
                          onChange={(event) =>
                            updateHookField(
                              behaviorValue,
                              "content",
                              event.currentTarget.value,
                            )
                          }
                          placeholder={t("monitor.management.contentPlaceholder")}
                          rows={2}
                          value={hook.content}
                        />
                      </Stack>
                    </Card>
                  );
                })}
              </SimpleGrid>
              <Card className="profile-save-bar" p="sm" withBorder>
                <Group justify="space-between" wrap="wrap">
                  <div>
                    <Text fw={650} size="sm">
                      {t("monitor.management.displayConfig")}
                    </Text>
                    <Text c="dimmed" mt={2} size="xs">
                      {isComplete
                        ? t("monitor.management.ready")
                        : t("monitor.management.remaining", {
                            count: draft.hooks.length - configuredBehaviorCount,
                          })}
                    </Text>
                  </div>
                  <Group>
                    {save.isSuccess && save.data.tool === activeTool ? (
                      <Badge color="teal" variant="light">
                        {t("monitor.management.saved")}
                      </Badge>
                    ) : null}
                    <Button
                      leftSection={<IconCheck aria-hidden="true" size={17} stroke={1.75} />}
                      loading={save.isPending}
                      onClick={() => save.mutate(draft)}
                    >
                      {t("monitor.management.save")}
                    </Button>
                  </Group>
                </Group>
              </Card>
            </Stack>
          </Tabs.Panel>
        ))}
      </Tabs>
    </Stack>
  );
}
