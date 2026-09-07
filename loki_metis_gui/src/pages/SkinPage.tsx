import {
  Alert,
  Badge,
  Button,
  Center,
  Group,
  Loader,
  Paper,
  Progress,
  SimpleGrid,
  Stack,
  Tabs,
  Text,
  Title,
} from "@mantine/core";
import {
  useMutation,
  useQuery,
  useQueryClient,
  type QueryKey,
} from "@tanstack/react-query";
import {
  IconAlertCircle,
  IconPalette,
  IconPlayerPlay,
  IconTrash,
} from "@tabler/icons-react";
import { useAtom } from "jotai";
import { useCallback, useEffect, useMemo, useState, type ReactElement } from "react";
import { useTranslation } from "react-i18next";

import {
  skinApi,
  skinHostAvailable,
  SkinHostError,
  type CodexInstance,
  type PreparedSkinImportBatch,
  type SkinAppearanceCheck,
  type SkinCreationPrompt,
  type SkinDescriptor,
  type SkinHostKind,
  type SkinReference,
} from "../api/skins";
import {
  SKIN_CATALOG_QUERY_KEY,
  skinInstancesQueryKey,
  skinRuntimeQueryKey,
  skinStatusQueryKey,
} from "../api/query-keys";
import { getMonitorCapabilities, getMonitorSettings } from "../api/monitor";
import {
  AppearanceDialog,
  CreateThemeDialog,
  ImportDialog,
  SkinConfirmDialog,
} from "../components/skins/SkinDialogs";
import { SkinCard } from "../components/skins/SkinCard";
import { SkinToolbar } from "../components/skins/SkinToolbar";
import {
  clearRememberedSkin,
  readRememberedSkin,
  rememberSkin,
} from "../lib/skin-preference";
import { resolveTargetInstance } from "../lib/skin-instances";
import { skinPageSessionAtom } from "../state/skin-page";

/** 把资源描述收敛成原生命令要求的精确引用。 */
function skinReference(skin: SkinDescriptor): SkinReference {
  return { id: skin.id, source: skin.source };
}

/** 比较两个可选皮肤引用是否指向同一份来源资源。 */
function sameSkin(left: SkinReference | null, right: SkinReference): boolean {
  return left?.id === right.id && left.source === right.source;
}

/** 以统一的启用/轮询/新鲜度策略订阅一个换皮宿主查询。 */
function useHostQuery<TData>(
  hostAvailable: boolean,
  queryKey: QueryKey,
  queryFn: () => Promise<TData>,
  intervalMs: number,
) {
  return useQuery({
    enabled: hostAvailable,
    queryFn,
    queryKey,
    refetchInterval: hostAvailable ? intervalMs : false,
    staleTime: intervalMs / 2,
  });
}

/** 渲染由统一 Agent 选择动态驱动的本机换皮资源库与宿主生命周期。 */
export function SkinPage(): ReactElement {
  const { t } = useTranslation();
  const queryClient = useQueryClient();
  const [session, setSession] = useAtom(skinPageSessionAtom);
  const [notice, setNotice] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [createOpened, setCreateOpened] = useState(false);
  const [creationPrompt, setCreationPrompt] = useState<SkinCreationPrompt | null>(null);
  const [importBatch, setImportBatch] = useState<PreparedSkinImportBatch | null>(null);
  const [importSelected, setImportSelected] = useState<string[]>([]);
  const [importProgress, setImportProgress] = useState<number | null>(null);
  const [appearance, setAppearance] = useState<{
    skin: SkinDescriptor;
    check: SkinAppearanceCheck;
  } | null>(null);
  const [restartSkin, setRestartSkin] = useState<SkinDescriptor | null>(null);
  const [convertSkin, setConvertSkin] = useState<SkinDescriptor | null>(null);
  const [deleteTargets, setDeleteTargets] = useState<SkinDescriptor[]>([]);
  const [selectedUserSkins, setSelectedUserSkins] = useState<SkinReference[]>([]);
  const [remembered, setRemembered] = useState<SkinReference | null>(null);

  const capabilities = useQuery({
    queryFn: getMonitorCapabilities,
    queryKey: ["monitor-capabilities"],
  });
  const settings = useQuery({
    queryFn: getMonitorSettings,
    queryKey: ["monitor-settings"],
  });
  const enabledTools = useMemo(
    () => new Set(settings.data?.enabledAiTools ?? []),
    [settings.data?.enabledAiTools],
  );
  const hostOptions = useMemo(
    () =>
      (capabilities.data?.aiTools ?? []).filter(
        (item): item is typeof item & { skinHost: SkinHostKind } =>
          enabledTools.has(item.tool) && item.skinHost != null,
      ),
    [capabilities.data?.aiTools, enabledTools],
  );
  const activeHost =
    hostOptions.find((item) => item.skinHost === session.selectedHost)?.skinHost ??
    hostOptions[0]?.skinHost ??
    null;
  const host = activeHost ?? "codex";
  const hostName =
    hostOptions.find((item) => item.skinHost === activeHost)?.name ??
    (host === "workBuddy" ? "WorkBuddy" : "Codex");
  const hostAvailable = skinHostAvailable() && activeHost !== null;
  const selectedInstanceId = session.selectedInstanceIds[host] ?? null;

  useEffect(() => {
    if (activeHost !== null && session.selectedHost !== activeHost) {
      setSession((value) => ({ ...value, selectedHost: activeHost }));
    }
  }, [activeHost, session.selectedHost, setSession]);

  useEffect(() => {
    setRemembered(activeHost === null ? null : readRememberedSkin(activeHost));
  }, [activeHost]);

  const catalog = useHostQuery(hostAvailable, SKIN_CATALOG_QUERY_KEY, skinApi.list, 5_000);
  const runtime = useHostQuery(
    hostAvailable,
    skinRuntimeQueryKey(host),
    () => skinApi.runtimeStatus(host),
    4_000,
  );
  const instances = useHostQuery(
    hostAvailable,
    skinInstancesQueryKey(host),
    () => skinApi.instances(host),
    4_000,
  );
  const status = useHostQuery(
    hostAvailable,
    skinStatusQueryKey(host),
    () => skinApi.status(host),
    4_000,
  );

  const action = useMutation<void, unknown, () => Promise<void>>({
    mutationFn: (operation) => operation(),
  });

  /** 统一显示脱敏宿主错误，不暴露文件路径或调试端点。 */
  const showError = useCallback(
    (cause: unknown): void => {
      setNotice(null);
      setError(cause instanceof SkinHostError ? cause.message : t("skins.error.unknown"));
    },
    [t],
  );

  /** 让换皮相关查询在一次真实变更后共同回到宿主权威状态。 */
  const refresh = useCallback(async (): Promise<void> => {
    await Promise.all([
      queryClient.invalidateQueries({ queryKey: SKIN_CATALOG_QUERY_KEY }),
      queryClient.invalidateQueries({ queryKey: skinStatusQueryKey(host) }),
      queryClient.invalidateQueries({ queryKey: skinRuntimeQueryKey(host) }),
      queryClient.invalidateQueries({ queryKey: skinInstancesQueryKey(host) }),
    ]);
  }, [host, queryClient]);

  /** 执行可见操作并收敛错误边界。 */
  const run = useCallback(
    async (operation: () => Promise<void>): Promise<void> => {
      setError(null);
      try {
        await action.mutateAsync(operation);
      } catch (cause) {
        showError(cause);
      }
    },
    [action.mutateAsync, showError],
  );

  useEffect(() => {
    const current = instances.data ?? [];
    const selectedStillExists = current.some((item) => item.id === selectedInstanceId);
    const onlyInstance = resolveTargetInstance(current, null);
    if (onlyInstance !== null && selectedInstanceId !== onlyInstance.id) {
      setSession((value) => ({
        ...value,
        selectedInstanceIds: { ...value.selectedInstanceIds, [host]: onlyInstance.id },
      }));
    } else if (!selectedStillExists && selectedInstanceId !== null) {
      setSession((value) => {
        const selectedInstanceIds = { ...value.selectedInstanceIds };
        delete selectedInstanceIds[host];
        return { ...value, selectedInstanceIds };
      });
    }
  }, [host, instances.data, selectedInstanceId, setSession]);

  useEffect(() => {
    if (!hostAvailable) return;
    let unlisten: (() => void) | undefined;
    void skinApi
      .onFileDrop((event) => {
        if (event.type !== "drop") return;
        setImportProgress(0);
        void run(async () => {
          const batch = await skinApi.prepareDroppedPaths(event.paths, (progress) => {
            if (progress.type === "started") setImportProgress(0);
            else
              setImportProgress((value) =>
                Math.min(100, (value ?? 0) + 100 / Math.max(1, event.paths.length)),
              );
          });
          setImportBatch(batch);
          setImportSelected(batch.items.map((item) => item.itemId));
          setImportProgress(null);
        });
      })
      .then((cleanup) => {
        unlisten = cleanup;
      });
    return () => unlisten?.();
  }, [hostAvailable]);

  const instanceList = instances.data ?? [];
  const selectedInstance = resolveTargetInstance(instanceList, selectedInstanceId);
  const activeSkin =
    selectedInstance?.activeSkin ??
    (status.data?.installed && status.data.skinId && status.data.source
      ? { id: status.data.skinId, source: status.data.source }
      : null);
  const filteredSkins = useMemo(() => {
    const term = session.search.trim().toLocaleLowerCase();
    return (catalog.data ?? []).filter((skin) => {
      if (session.userOnly && skin.source !== "user") return false;
      return (
        !term || `${skin.name} ${skin.author} ${skin.id}`.toLocaleLowerCase().includes(term)
      );
    });
  }, [catalog.data, session.search, session.userOnly]);
  const rememberedDescriptor = useMemo(
    () =>
      (catalog.data ?? []).find(
        (skin) => remembered !== null && sameSkin(remembered, skinReference(skin)),
      ),
    [catalog.data, remembered],
  );
  const selectedSkinKeys = useMemo(
    () => new Set(selectedUserSkins.map((entry) => `${entry.source}:${entry.id}`)),
    [selectedUserSkins],
  );

  /** 在实例明确且无需重启时执行皮肤安装。 */
  const installOnInstance = useCallback(
    async (
      skin: SkinDescriptor,
      target: CodexInstance | null,
      allowMismatch = false,
    ): Promise<void> => {
      const result = await skinApi.install(
        host,
        skinReference(skin),
        allowMismatch,
        target?.id ?? null,
      );
      if (result.type === "needsConfirmation") {
        setAppearance({ check: result.check, skin });
        return;
      }
      const reference = skinReference(skin);
      rememberSkin(host, reference);
      setRemembered(reference);
      setSession((value) => ({
        ...value,
        restoreDismissedHosts: { ...value.restoreDismissedHosts, [host]: false },
      }));
      setNotice(t("skins.notice.applied", { name: skin.name }));
      await refresh();
    },
    [host, refresh, setSession, t],
  );

  /** 解析唯一目标并在可能影响 Codex 会话时先进入确认弹窗。 */
  const requestInstall = useCallback(
    async (skin: SkinDescriptor): Promise<void> => {
      let current = instanceList;
      if (current.length === 0) {
        await skinApi.launchHost(host);
        current = await queryClient.fetchQuery({
          queryFn: () => skinApi.instances(host),
          queryKey: skinInstancesQueryKey(host),
        });
      }
      const target = resolveTargetInstance(current, selectedInstanceId);
      if (target === null)
        throw new SkinHostError(
          "skin.host_instance_selection_required",
          t("skins.error.choose_instance"),
        );
      if (target.state === "runningWithoutCdp") {
        setRestartSkin(skin);
        return;
      }
      await installOnInstance(skin, target);
    },
    [host, instanceList, installOnInstance, queryClient, selectedInstanceId, t],
  );

  /** 从原生文件选择器预检一个有界 ZIP 批次。 */
  const prepareImport = useCallback(async (): Promise<void> => {
    setImportProgress(0);
    const batch = await skinApi.prepareImport((progress) => {
      if (progress.type === "started") setImportProgress(0);
      else setImportProgress((value) => Math.min(95, (value ?? 0) + 12));
    });
    setImportProgress(null);
    if (batch === null) return;
    setImportBatch(batch);
    setImportSelected(batch.items.map((item) => item.itemId));
  }, []);

  /** 稳定的资源卡回调集合，避免轮询刷新导致整批卡片重渲染。 */
  const handleApply = useCallback(
    (item: SkinDescriptor) => void run(() => requestInstall(item)),
    [run, requestInstall],
  );
  const handleConvert = useCallback((item: SkinDescriptor) => setConvertSkin(item), []);
  const handleDelete = useCallback((item: SkinDescriptor) => setDeleteTargets([item]), []);
  const handleExport = useCallback(
    (item: SkinDescriptor) =>
      void run(async () => {
        if (await skinApi.exportPackage(skinReference(item)))
          setNotice(t("skins.notice.exported"));
      }),
    [run, t],
  );
  const handleOpen = useCallback(
    (item: SkinDescriptor) => void run(() => skinApi.openDirectory(skinReference(item))),
    [run],
  );
  const handleSelect = useCallback(
    (item: SkinDescriptor, selected: boolean) =>
      setSelectedUserSkins((value) =>
        selected
          ? [
              ...value.filter((entry) => !sameSkin(entry, skinReference(item))),
              skinReference(item),
            ]
          : value.filter((entry) => !sameSkin(entry, skinReference(item))),
      ),
    [],
  );
  const handleStop = useCallback(
    () =>
      void run(async () => {
        await skinApi.uninstall(host, selectedInstance?.id ?? null);
        clearRememberedSkin(host);
        setRemembered(null);
        setNotice(t("skins.notice.stopped"));
        await refresh();
      }),
    [host, refresh, run, selectedInstance, t],
  );

  const runtimeLabel = runtime.data?.state ?? "stopped";
  return (
    <Stack data-testid="skin-page" gap="lg">
      <Group align="flex-start" justify="space-between">
        <Stack gap={4}>
          <Title order={1}>{t("skins.title")}</Title>
          <Text c="dimmed">{t("skins.description")}</Text>
        </Stack>
        <Badge
          color={
            runtimeLabel === "ready"
              ? "teal"
              : runtimeLabel === "runningWithoutCdp"
                ? "yellow"
                : "gray"
          }
          size="lg"
          variant="light"
        >
          {t(`skins.runtime.${runtimeLabel}`, { host: hostName })}
        </Badge>
      </Group>

      {hostOptions.length > 0 ? (
        <Tabs
          aria-label={t("skins.host.tabs")}
          onChange={(value) =>
            setSession((current) => ({
              ...current,
              selectedHost: value as SkinHostKind | null,
            }))
          }
          value={activeHost}
        >
          <Tabs.List>
            {hostOptions.map((item) => (
              <Tabs.Tab key={item.skinHost} value={item.skinHost}>
                {item.name}
              </Tabs.Tab>
            ))}
          </Tabs.List>
        </Tabs>
      ) : null}

      {!skinHostAvailable() ? (
        <Alert icon={<IconAlertCircle size={18} />} title={t("skins.browser.title")}>
          {t("skins.browser.description")}
        </Alert>
      ) : null}
      {skinHostAvailable() &&
      !capabilities.isPending &&
      !settings.isPending &&
      hostOptions.length === 0 ? (
        <Alert icon={<IconAlertCircle size={18} />} title={t("skins.host.empty_title")}>
          {t("skins.host.empty_description")}
        </Alert>
      ) : null}
      {error !== null ? (
        <Alert
          color="red"
          icon={<IconAlertCircle size={18} />}
          onClose={() => setError(null)}
          role="alert"
          title={t("skins.error.title")}
          withCloseButton
        >
          {error}
        </Alert>
      ) : null}
      {notice !== null ? (
        <Alert color="teal" onClose={() => setNotice(null)} role="status" withCloseButton>
          {notice}
        </Alert>
      ) : null}
      {importProgress !== null ? <Progress animated value={importProgress} /> : null}

      <Paper className="surface-card" p="md" radius="lg" withBorder>
        <Stack gap="md">
          <SkinToolbar
            busy={action.isPending}
            hostName={hostName}
            hostAvailable={hostAvailable}
            instances={instanceList}
            onCreate={() => setCreateOpened(true)}
            onImport={() => void run(prepareImport)}
            onRefresh={() => void run(refresh)}
            onSearchChange={(search) => setSession((value) => ({ ...value, search }))}
            onSelectedInstanceChange={(selectedInstanceId) =>
              setSession((value) => {
                const selectedInstanceIds = { ...value.selectedInstanceIds };
                if (selectedInstanceId === null) delete selectedInstanceIds[host];
                else selectedInstanceIds[host] = selectedInstanceId;
                return { ...value, selectedInstanceIds };
              })
            }
            onUserOnlyChange={(userOnly) => setSession((value) => ({ ...value, userOnly }))}
            search={session.search}
            selectedInstanceId={selectedInstanceId}
            userOnly={session.userOnly}
          />
          <Group justify="space-between">
            <Text c="dimmed" size="sm">
              {t("skins.catalog.count", { count: filteredSkins.length })}
            </Text>
            <Group gap="xs">
              {runtimeLabel === "stopped" ? (
                <Button
                  disabled={!hostAvailable}
                  leftSection={<IconPlayerPlay size={17} />}
                  loading={action.isPending}
                  onClick={() =>
                    void run(async () => {
                      await skinApi.launchHost(host);
                      await refresh();
                    })
                  }
                  size="xs"
                  variant="light"
                >
                  {t("skins.action.launch", { host: hostName })}
                </Button>
              ) : null}
              {selectedUserSkins.length > 0 ? (
                <Button
                  color="red"
                  leftSection={<IconTrash size={17} />}
                  onClick={() =>
                    setDeleteTargets(
                      (catalog.data ?? []).filter((skin) =>
                        selectedSkinKeys.has(`${skin.source}:${skin.id}`),
                      ),
                    )
                  }
                  size="xs"
                  variant="light"
                >
                  {t("skins.action.delete_selected", { count: selectedUserSkins.length })}
                </Button>
              ) : null}
            </Group>
          </Group>
        </Stack>
      </Paper>

      {rememberedDescriptor &&
      activeSkin === null &&
      !session.restoreDismissedHosts[host] ? (
        <Alert
          color="violet"
          icon={<IconPalette size={18} />}
          title={t("skins.restore.title")}
        >
          <Group justify="space-between">
            <Text size="sm">
              {t("skins.restore.description", { name: rememberedDescriptor.name })}
            </Text>
            <Group gap="xs">
              <Button
                onClick={() => void run(() => requestInstall(rememberedDescriptor))}
                size="xs"
              >
                {t("skins.restore.action")}
              </Button>
              <Button
                onClick={() =>
                  setSession((value) => ({
                    ...value,
                    restoreDismissedHosts: {
                      ...value.restoreDismissedHosts,
                      [host]: true,
                    },
                  }))
                }
                size="xs"
                variant="subtle"
              >
                {t("skins.restore.dismiss")}
              </Button>
            </Group>
          </Group>
        </Alert>
      ) : null}

      {catalog.isLoading ? (
        <Center py="xl">
          <Loader />
        </Center>
      ) : filteredSkins.length === 0 ? (
        <Center py={72}>
          <Stack align="center">
            <IconPalette color="var(--mantine-color-dimmed)" size={42} />
            <Text c="dimmed">{t("skins.catalog.empty")}</Text>
          </Stack>
        </Center>
      ) : (
        <SimpleGrid cols={{ base: 1, sm: 2, lg: 3, xl: 4 }} spacing="lg">
          {filteredSkins.map((skin) => (
            <SkinCard
              active={sameSkin(activeSkin, skinReference(skin))}
              busy={action.isPending || !hostAvailable}
              key={`${skin.source}:${skin.id}`}
              onApply={handleApply}
              onConvert={handleConvert}
              onDelete={handleDelete}
              onExport={handleExport}
              onOpen={handleOpen}
              onSelect={handleSelect}
              onStop={handleStop}
              selected={selectedSkinKeys.has(`${skin.source}:${skin.id}`)}
              skin={skin}
            />
          ))}
        </SimpleGrid>
      )}

      <CreateThemeDialog
        onClose={() => setCreateOpened(false)}
        onCreate={(name, author) =>
          void run(async () => {
            const created = await skinApi.createTheme(name, author);
            setCreateOpened(false);
            setNotice(t("skins.notice.created", { name: created.name }));
            await refresh();
          })
        }
        onLoadPrompt={() =>
          void run(async () => setCreationPrompt(await skinApi.creationPrompt()))
        }
        opened={createOpened}
        pending={action.isPending}
        prompt={creationPrompt}
        promptPending={action.isPending}
      />
      <ImportDialog
        batch={importBatch}
        onCancel={() =>
          void run(async () => {
            if (importBatch) await skinApi.cancelImport(importBatch.token);
            setImportBatch(null);
          })
        }
        onCommit={() =>
          void run(async () => {
            if (!importBatch) return;
            const result = await skinApi.commitImport(importBatch.token, importSelected);
            setImportBatch(null);
            setNotice(t("skins.notice.imported", { count: result.installed.length }));
            await refresh();
          })
        }
        onSelectedChange={setImportSelected}
        pending={action.isPending}
        selected={importSelected}
      />
      <AppearanceDialog
        check={appearance?.check ?? null}
        onCancel={() => setAppearance(null)}
        onConfirm={() =>
          void run(async () => {
            if (!appearance) return;
            const pending = appearance;
            setAppearance(null);
            await installOnInstance(pending.skin, selectedInstance, true);
          })
        }
        pending={action.isPending}
      />
      <SkinConfirmDialog
        description={t("skins.restart.description", {
          name: selectedInstance?.label ?? "",
        })}
        onCancel={() => setRestartSkin(null)}
        onConfirm={() =>
          void run(async () => {
            if (!restartSkin || !selectedInstance) return;
            const skin = restartSkin;
            setRestartSkin(null);
            const restarted = await skinApi.restartInstance(host, selectedInstance.id);
            await installOnInstance(skin, restarted);
          })
        }
        opened={restartSkin !== null}
        pending={action.isPending}
        title={t("skins.restart.title")}
      />
      <SkinConfirmDialog
        description={t("skins.convert.description", { name: convertSkin?.name ?? "" })}
        onCancel={() => setConvertSkin(null)}
        onConfirm={() =>
          void run(async () => {
            if (!convertSkin) return;
            const result = await skinApi.convertToTheme(skinReference(convertSkin));
            setConvertSkin(null);
            setNotice(t("skins.notice.converted", { name: result.theme.name }));
            await refresh();
          })
        }
        opened={convertSkin !== null}
        pending={action.isPending}
        title={t("skins.convert.title")}
      />
      <SkinConfirmDialog
        confirmColor="red"
        description={t("skins.delete.description", { count: deleteTargets.length })}
        onCancel={() => setDeleteTargets([])}
        onConfirm={() =>
          void run(async () => {
            const targets = deleteTargets;
            const result = await skinApi.deleteMany(targets.map(skinReference));
            setDeleteTargets([]);
            setSelectedUserSkins([]);
            await refresh();
            const firstFailure = result.failed[0];
            if (firstFailure) {
              throw new SkinHostError(
                firstFailure.code,
                firstFailure.message,
                result.failed.map((failure) => `${failure.skin.id}: ${failure.message}`),
              );
            }
            setNotice(t("skins.notice.deleted", { count: result.deleted.length }));
          })
        }
        opened={deleteTargets.length > 0}
        pending={action.isPending}
        title={t("skins.delete.title")}
      />
    </Stack>
  );
}
