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
  Text,
  Title,
} from "@mantine/core";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import {
  IconAlertCircle,
  IconPalette,
  IconPlayerPlay,
  IconTrash,
} from "@tabler/icons-react";
import { useAtom } from "jotai";
import { useEffect, useMemo, useState, type ReactElement } from "react";
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
  type SkinReference,
} from "../api/skins";
import {
  CODEX_INSTANCES_QUERY_KEY,
  CODEX_RUNTIME_QUERY_KEY,
  SKIN_CATALOG_QUERY_KEY,
  SKIN_STATUS_QUERY_KEY,
} from "../api/query-keys";
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
import { skinPageSessionAtom } from "../state/skin-page";

/** 把资源描述收敛成原生命令要求的精确引用。 */
function skinReference(skin: SkinDescriptor): SkinReference {
  return { id: skin.id, source: skin.source };
}

/** 比较两个可选皮肤引用是否指向同一份来源资源。 */
function sameSkin(left: SkinReference | null, right: SkinReference): boolean {
  return left?.id === right.id && left.source === right.source;
}

/** 渲染完整的本机 Codex 换皮资源库、目标实例与受控生命周期。 */
export function SkinPage(): ReactElement {
  const { t } = useTranslation();
  const queryClient = useQueryClient();
  const hostAvailable = skinHostAvailable();
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
  const [remembered, setRemembered] = useState<SkinReference | null>(() =>
    readRememberedSkin(),
  );

  const catalog = useQuery({
    enabled: hostAvailable,
    queryFn: skinApi.list,
    queryKey: SKIN_CATALOG_QUERY_KEY,
    refetchInterval: hostAvailable ? 5_000 : false,
  });
  const runtime = useQuery({
    enabled: hostAvailable,
    queryFn: skinApi.runtimeStatus,
    queryKey: CODEX_RUNTIME_QUERY_KEY,
    refetchInterval: hostAvailable ? 4_000 : false,
  });
  const instances = useQuery({
    enabled: hostAvailable,
    queryFn: skinApi.instances,
    queryKey: CODEX_INSTANCES_QUERY_KEY,
    refetchInterval: hostAvailable ? 4_000 : false,
  });
  const status = useQuery({
    enabled: hostAvailable,
    queryFn: skinApi.status,
    queryKey: SKIN_STATUS_QUERY_KEY,
    refetchInterval: hostAvailable ? 4_000 : false,
  });

  const action = useMutation<void, unknown, () => Promise<void>>({
    mutationFn: (operation) => operation(),
  });

  /** 统一显示脱敏宿主错误，不暴露文件路径或调试端点。 */
  const showError = (cause: unknown): void => {
    setNotice(null);
    setError(cause instanceof SkinHostError ? cause.message : t("skins.error.unknown"));
  };

  /** 让换皮相关查询在一次真实变更后共同回到宿主权威状态。 */
  const refresh = async (): Promise<void> => {
    await Promise.all([
      queryClient.invalidateQueries({ queryKey: SKIN_CATALOG_QUERY_KEY }),
      queryClient.invalidateQueries({ queryKey: SKIN_STATUS_QUERY_KEY }),
      queryClient.invalidateQueries({ queryKey: CODEX_RUNTIME_QUERY_KEY }),
      queryClient.invalidateQueries({ queryKey: CODEX_INSTANCES_QUERY_KEY }),
    ]);
  };

  /** 执行可见操作并收敛错误边界。 */
  const run = async (operation: () => Promise<void>): Promise<void> => {
    setError(null);
    try {
      await action.mutateAsync(operation);
    } catch (cause) {
      showError(cause);
    }
  };

  useEffect(() => {
    const current = instances.data ?? [];
    const selectedStillExists = current.some(
      (item) => item.id === session.selectedInstanceId,
    );
    const onlyInstance = current.length === 1 ? (current.at(0) ?? null) : null;
    if (onlyInstance !== null && session.selectedInstanceId !== onlyInstance.id) {
      setSession((value) => ({ ...value, selectedInstanceId: onlyInstance.id }));
    } else if (!selectedStillExists && session.selectedInstanceId !== null) {
      setSession((value) => ({ ...value, selectedInstanceId: null }));
    }
  }, [instances.data, session.selectedInstanceId, setSession]);

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
  const selectedInstance =
    instanceList.find((item) => item.id === session.selectedInstanceId) ??
    (instanceList.length === 1 ? (instanceList.at(0) ?? null) : null);
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
  const rememberedDescriptor = (catalog.data ?? []).find(
    (skin) => remembered !== null && sameSkin(remembered, skinReference(skin)),
  );

  /** 在实例明确且无需重启时执行皮肤安装。 */
  const installOnInstance = async (
    skin: SkinDescriptor,
    target: CodexInstance | null,
    allowMismatch = false,
  ): Promise<void> => {
    const result = await skinApi.install(
      skinReference(skin),
      allowMismatch,
      target?.id ?? null,
    );
    if (result.type === "needsConfirmation") {
      setAppearance({ check: result.check, skin });
      return;
    }
    const reference = skinReference(skin);
    rememberSkin(reference);
    setRemembered(reference);
    setSession((value) => ({ ...value, restoreDismissed: false }));
    setNotice(t("skins.notice.applied", { name: skin.name }));
    await refresh();
  };

  /** 解析唯一目标并在可能影响 Codex 会话时先进入确认弹窗。 */
  const requestInstall = async (skin: SkinDescriptor): Promise<void> => {
    let current = instanceList;
    if (current.length === 0) {
      await skinApi.launchCodex();
      current = await skinApi.instances();
    }
    const target =
      current.find((item) => item.id === session.selectedInstanceId) ??
      (current.length === 1 ? (current.at(0) ?? null) : null);
    if (target === null)
      throw new SkinHostError(
        "skin.codex_instance_selection_required",
        t("skins.error.choose_instance"),
      );
    if (target.state === "runningWithoutCdp") {
      setRestartSkin(skin);
      return;
    }
    await installOnInstance(skin, target);
  };

  /** 从原生文件选择器预检一个有界 ZIP 批次。 */
  const prepareImport = async (): Promise<void> => {
    setImportProgress(0);
    const batch = await skinApi.prepareImport((progress) => {
      if (progress.type === "started") setImportProgress(0);
      else setImportProgress((value) => Math.min(95, (value ?? 0) + 12));
    });
    setImportProgress(null);
    if (batch === null) return;
    setImportBatch(batch);
    setImportSelected(batch.items.map((item) => item.itemId));
  };

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
          {t(`skins.runtime.${runtimeLabel}`)}
        </Badge>
      </Group>

      {!hostAvailable ? (
        <Alert icon={<IconAlertCircle size={18} />} title={t("skins.browser.title")}>
          {t("skins.browser.description")}
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
            hostAvailable={hostAvailable}
            instances={instanceList}
            onCreate={() => setCreateOpened(true)}
            onImport={() => void run(prepareImport)}
            onRefresh={() => void run(refresh)}
            onSearchChange={(search) => setSession((value) => ({ ...value, search }))}
            onSelectedInstanceChange={(selectedInstanceId) =>
              setSession((value) => ({ ...value, selectedInstanceId }))
            }
            onUserOnlyChange={(userOnly) => setSession((value) => ({ ...value, userOnly }))}
            search={session.search}
            selectedInstanceId={session.selectedInstanceId}
            userOnly={session.userOnly}
          />
          <Group justify="space-between">
            <Text c="dimmed" size="sm">
              {t("skins.catalog.count", { count: filteredSkins.length })}
            </Text>
            <Group gap="xs">
              {runtimeLabel === "stopped" ? (
                <Button
                  leftSection={<IconPlayerPlay size={17} />}
                  loading={action.isPending}
                  onClick={() =>
                    void run(async () => {
                      await skinApi.launchCodex();
                      await refresh();
                    })
                  }
                  size="xs"
                  variant="light"
                >
                  {t("skins.action.launch")}
                </Button>
              ) : null}
              {selectedUserSkins.length > 0 ? (
                <Button
                  color="red"
                  leftSection={<IconTrash size={17} />}
                  onClick={() =>
                    setDeleteTargets(
                      (catalog.data ?? []).filter((skin) =>
                        selectedUserSkins.some((selected) =>
                          sameSkin(selected, skinReference(skin)),
                        ),
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

      {rememberedDescriptor && activeSkin === null && !session.restoreDismissed ? (
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
                  setSession((value) => ({ ...value, restoreDismissed: true }))
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
              busy={action.isPending}
              key={`${skin.source}:${skin.id}`}
              onApply={(item) => void run(() => requestInstall(item))}
              onConvert={(item) => setConvertSkin(item)}
              onDelete={(item) => setDeleteTargets([item])}
              onExport={(item) =>
                void run(async () => {
                  if (await skinApi.exportPackage(skinReference(item)))
                    setNotice(t("skins.notice.exported"));
                })
              }
              onOpen={(item) => void run(() => skinApi.openDirectory(skinReference(item)))}
              onSelect={(item, selected) =>
                setSelectedUserSkins((value) =>
                  selected
                    ? [
                        ...value.filter((entry) => !sameSkin(entry, skinReference(item))),
                        skinReference(item),
                      ]
                    : value.filter((entry) => !sameSkin(entry, skinReference(item))),
                )
              }
              onStop={() =>
                void run(async () => {
                  await skinApi.uninstall(selectedInstance?.id ?? null);
                  clearRememberedSkin();
                  setRemembered(null);
                  setNotice(t("skins.notice.stopped"));
                  await refresh();
                })
              }
              selected={selectedUserSkins.some((entry) =>
                sameSkin(entry, skinReference(skin)),
              )}
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
            const restarted = await skinApi.restartInstance(selectedInstance.id);
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
