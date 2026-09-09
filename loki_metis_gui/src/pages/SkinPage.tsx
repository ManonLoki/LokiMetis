import {
  Alert,
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
} from "@mantine/core";
import { useMutation, useQueryClient } from "@tanstack/react-query";
import { IconAlertCircle, IconPalette, IconTrash } from "@tabler/icons-react";
import {
  useCallback,
  useEffect,
  useMemo,
  useRef,
  useState,
  type ReactElement,
} from "react";
import { useTranslation } from "react-i18next";

import {
  skinApi,
  skinHostAvailable,
  SkinHostError,
  type SkinCreationPrompt,
  type SkinDescriptor,
  type SkinHostKind,
  type SkinReference,
} from "../api/skins";
import {
  SKIN_CATALOG_QUERY_KEY,
  skinInstancesQueryKey,
  skinStatusQueryKey,
} from "../api/query-keys";
import {
  AppearanceDialog,
  CreateThemeDialog,
  ImportDialog,
  SkinConfirmDialog,
  ThirdPartyCodeDialog,
} from "../components/skins/SkinDialogs";
import { SkinCard } from "../components/skins/SkinCard";
import { SkinToolbar } from "../components/skins/SkinToolbar";
import { clearRememberedSkin } from "../lib/skin-preference";
import { useSkinHostQueries } from "./useSkinHostQueries";
import { skinReference, useSkinHostActions } from "./useSkinHostActions";
import { useSkinImportController } from "./useSkinImportController";

/** 比较两个可选皮肤引用是否指向同一份来源资源。 */
function sameSkin(left: SkinReference | null, right: SkinReference): boolean {
  return left?.id === right.id && left.source === right.source;
}

/** 将一次性第三方代码确认绑定到发起操作时的宿主，防止切换标签后错投。 */
interface ThirdPartyCodeRequest {
  host: SkinHostKind;
  skin: SkinDescriptor;
}

/** 渲染由统一 Agent 选择动态驱动的本机换皮资源库与宿主生命周期。 */
export function SkinPage(): ReactElement {
  const { t } = useTranslation();
  const queryClient = useQueryClient();
  const {
    activeHost,
    capabilities,
    catalog,
    host,
    hostAvailable,
    hostOptions,
    hostStateReady,
    instances,
    queryFailure,
    queryRefreshFailure,
    session,
    setSession,
    settings,
    status,
  } = useSkinHostQueries();
  const [notice, setNotice] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [createOpened, setCreateOpened] = useState(false);
  const [creationPrompt, setCreationPrompt] = useState<SkinCreationPrompt | null>(null);
  const [thirdPartyCodeRequest, setThirdPartyCodeRequest] =
    useState<ThirdPartyCodeRequest | null>(null);
  const [convertSkin, setConvertSkin] = useState<SkinDescriptor | null>(null);
  const [deleteTargets, setDeleteTargets] = useState<SkinDescriptor[]>([]);
  const [selectedUserSkins, setSelectedUserSkins] = useState<SkinReference[]>([]);
  const mounted = useRef(true);

  useEffect(() => {
    mounted.current = true;
    return () => {
      mounted.current = false;
    };
  }, []);

  const action = useMutation<void, unknown, () => Promise<void>>({
    mutationFn: (operation) => operation(),
  });

  /** 统一显示脱敏宿主错误，不暴露文件路径或调试端点。 */
  const showError = useCallback(
    (cause: unknown): void => {
      if (!mounted.current) return;
      setNotice(null);
      setError(cause instanceof SkinHostError ? cause.message : t("skins.error.unknown"));
    },
    [t],
  );

  /** 让换皮相关查询在一次真实变更后共同回到宿主权威状态。 */
  const refreshHost = useCallback(
    async (targetHost: SkinHostKind): Promise<void> => {
      await Promise.all([
        queryClient.invalidateQueries({ queryKey: SKIN_CATALOG_QUERY_KEY }),
        queryClient.invalidateQueries({ queryKey: skinStatusQueryKey(targetHost) }),
        queryClient.invalidateQueries({ queryKey: skinInstancesQueryKey(targetHost) }),
      ]);
    },
    [queryClient],
  );
  const refresh = useCallback(() => refreshHost(host), [host, refreshHost]);

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

  const {
    importBatch,
    importProgress,
    importSelected,
    prepareImport,
    setImportBatch,
    setImportSelected,
  } = useSkinImportController({
    enabled: hostStateReady,
    mounted,
    run,
    showError,
  });

  const instanceList = instances.data ?? [];
  const {
    appearance,
    installOnInstance,
    remembered,
    requestInstall,
    restartAndInstall,
    restartRequest,
    selectedInstance,
    setAppearance,
    setRemembered,
    setRestartRequest,
  } = useSkinHostActions({
    activeHost,
    host,
    hostStateReady,
    instanceList,
    refreshHost,
    setNotice,
  });
  const activeSkin =
    selectedInstance?.activeSkin ??
    (status.data?.installed && status.data.skinId && status.data.source
      ? { id: status.data.skinId, source: status.data.source }
      : null);
  const filteredSkins = useMemo(() => {
    const term = session.search.trim().toLocaleLowerCase();
    return (catalog.data ?? []).filter((skin) => {
      return (
        !term || `${skin.name} ${skin.author} ${skin.id}`.toLocaleLowerCase().includes(term)
      );
    });
  }, [catalog.data, session.search]);
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

  /** 稳定的资源卡回调集合，避免轮询刷新导致整批卡片重渲染。 */
  const handleApply = useCallback(
    (item: SkinDescriptor) => {
      if (item.packageType === "legacySkin") {
        setThirdPartyCodeRequest({ host, skin: item });
        return;
      }
      void run(() => requestInstall(item, false));
    },
    [host, run, requestInstall],
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
  const hostTransitionLocked =
    action.isPending ||
    thirdPartyCodeRequest !== null ||
    appearance !== null ||
    restartRequest !== null;

  return (
    <Stack data-testid="skin-page" gap="lg">
      {hostOptions.length > 0 ? (
        <Tabs
          aria-label={t("skins.host.tabs")}
          onChange={(value) => {
            if (hostTransitionLocked) return;
            setSession((current) => ({
              ...current,
              selectedHost: value as SkinHostKind | null,
            }));
          }}
          value={activeHost}
        >
          <Tabs.List>
            {hostOptions.map((item) => (
              <Tabs.Tab
                disabled={hostTransitionLocked}
                key={item.skinHost}
                value={item.skinHost}
              >
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
      queryFailure === null &&
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
      {queryFailure !== null || queryRefreshFailure !== null ? (
        <Alert color="red" icon={<IconAlertCircle size={18} />} role="alert">
          <Group justify="space-between">
            <Text size="sm">
              {(queryFailure ?? queryRefreshFailure) instanceof SkinHostError
                ? (queryFailure ?? queryRefreshFailure)?.message
                : t("skins.error.unknown")}
            </Text>
            <Button
              onClick={() =>
                void Promise.all([
                  capabilities.refetch(),
                  settings.refetch(),
                  ...(hostAvailable
                    ? [catalog.refetch(), instances.refetch(), status.refetch()]
                    : []),
                ])
              }
              size="xs"
              variant="light"
            >
              {t("common.retry")}
            </Button>
          </Group>
        </Alert>
      ) : null}
      {importProgress !== null ? <Progress animated value={importProgress} /> : null}

      <Paper className="surface-card" p="md" radius="lg" withBorder>
        <Stack gap="md">
          <SkinToolbar
            busy={action.isPending}
            hostAvailable={hostStateReady}
            onCreate={() => setCreateOpened(true)}
            onImport={() => void run(prepareImport)}
            onRefresh={() => void run(refresh)}
            onSearchChange={(search) => setSession((value) => ({ ...value, search }))}
            search={session.search}
          />
          {selectedUserSkins.length > 0 ? (
            <Group gap="xs" justify="flex-end">
              <Button
                color="red"
                disabled={!hostStateReady || action.isPending}
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
            </Group>
          ) : null}
        </Stack>
      </Paper>

      {rememberedDescriptor &&
      hostStateReady &&
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
              <Button onClick={() => handleApply(rememberedDescriptor)} size="xs">
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

      {capabilities.isPending || settings.isPending ? (
        <Center py="xl">
          <Loader />
        </Center>
      ) : !hostAvailable || queryFailure !== null ? null : catalog.isLoading ? (
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
              busy={action.isPending || !hostStateReady}
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
        blocked={!hostStateReady}
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
        blocked={!hostStateReady}
        onCancel={() =>
          void run(async () => {
            if (importBatch) await skinApi.cancelImport(importBatch.token);
            setImportBatch(null);
          })
        }
        onCommit={(allowThirdPartyCode) =>
          void run(async () => {
            if (!importBatch) return;
            const result = await skinApi.commitImport(
              importBatch.token,
              importSelected,
              allowThirdPartyCode,
            );
            setImportBatch(null);
            setNotice(t("skins.notice.imported", { count: result.installed.length }));
            await refresh();
          })
        }
        onSelectedChange={setImportSelected}
        pending={action.isPending}
        selected={importSelected}
      />
      <ThirdPartyCodeDialog
        blocked={!hostStateReady}
        hostName={
          hostOptions.find((item) => item.skinHost === thirdPartyCodeRequest?.host)?.name ??
          (thirdPartyCodeRequest?.host === "workBuddy" ? "WorkBuddy" : "Codex")
        }
        onCancel={() => setThirdPartyCodeRequest(null)}
        onConfirm={() =>
          void run(async () => {
            if (!thirdPartyCodeRequest) return;
            const pending = thirdPartyCodeRequest;
            setThirdPartyCodeRequest(null);
            await requestInstall(pending.skin, true, pending.host);
          })
        }
        opened={thirdPartyCodeRequest !== null}
        pending={action.isPending}
        skinName={thirdPartyCodeRequest?.skin.name ?? ""}
      />
      <AppearanceDialog
        blocked={!hostStateReady}
        check={appearance?.check ?? null}
        onCancel={() => setAppearance(null)}
        onConfirm={() =>
          void run(async () => {
            if (!appearance) return;
            const pending = appearance;
            setAppearance(null);
            await installOnInstance(
              pending.host,
              pending.skin,
              pending.target,
              pending.allowThirdPartyCode,
              true,
              pending.allowWorkBuddyRecovery,
            );
          })
        }
        pending={action.isPending}
      />
      <SkinConfirmDialog
        blocked={!hostStateReady}
        description={t(
          restartRequest?.mode === "recoverWindowsWorkBuddy"
            ? "skins.restart.workbuddy_description"
            : "skins.restart.description",
          { name: restartRequest?.instanceLabel ?? "" },
        )}
        onCancel={() => setRestartRequest(null)}
        onConfirm={() =>
          void run(async () => {
            if (!restartRequest) return;
            const pending = restartRequest;
            setRestartRequest(null);
            if (pending.mode === "recoverWindowsWorkBuddy") {
              await installOnInstance(
                pending.host,
                pending.skin,
                null,
                pending.allowThirdPartyCode,
                false,
                true,
              );
              return;
            }
            if (!pending.instanceId) return;
            await restartAndInstall(
              pending.host,
              pending.skin,
              pending.instanceId,
              pending.allowThirdPartyCode,
            );
          })
        }
        opened={restartRequest !== null}
        pending={action.isPending}
        title={t(
          restartRequest?.mode === "recoverWindowsWorkBuddy"
            ? "skins.restart.workbuddy_title"
            : "skins.restart.title",
        )}
      />
      <SkinConfirmDialog
        blocked={!hostStateReady}
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
        blocked={!hostStateReady}
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
