import { useQueryClient } from "@tanstack/react-query";
import { useSetAtom } from "jotai";
import {
  useCallback,
  useEffect,
  useMemo,
  useRef,
  useState,
  type Dispatch,
  type SetStateAction,
} from "react";
import { useTranslation } from "react-i18next";

import { skinInstancesQueryKey } from "../api/query-keys";
import {
  skinApi,
  SkinHostError,
  type CodexInstance,
  type SkinAppearanceCheck,
  type SkinDescriptor,
  type SkinHostKind,
  type SkinReference,
} from "../api/skins";
import {
  needsWorkBuddyCdpRecovery,
  resolveSoleTargetInstance,
} from "../lib/skin-instances";
import { readRememberedSkin, rememberSkin } from "../lib/skin-preference";
import { skinPageSessionAtom } from "../state/skin-page";

/** 把资源描述收敛成原生命令要求的精确引用。 */
export function skinReference(skin: SkinDescriptor): SkinReference {
  return { id: skin.id, source: skin.source };
}

/** 保存需要用户确认的外观不匹配安装请求。 */
export interface SkinAppearanceRequest {
  allowThirdPartyCode: boolean;
  host: SkinHostKind;
  skin: SkinDescriptor;
  check: SkinAppearanceCheck;
  target: CodexInstance | null;
  allowWorkBuddyRecovery: boolean;
}

/** 待用户确认的宿主重启请求：记录目标宿主、实例与本次皮肤。 */
export interface PendingHostRestart {
  allowThirdPartyCode: boolean;
  host: SkinHostKind;
  instanceId: string | null;
  instanceLabel: string;
  mode: "restartSelected" | "recoverWindowsWorkBuddy";
  skin: SkinDescriptor;
}

/** 宿主动作编排所需的稳定输入；页面只负责展示确认结果。 */
interface SkinHostActionsOptions {
  activeHost: SkinHostKind | null;
  host: SkinHostKind;
  hostStateReady: boolean;
  instanceList: CodexInstance[];
  refreshHost: (host: SkinHostKind) => Promise<void>;
  setNotice: Dispatch<SetStateAction<string | null>>;
}

/** 把一次安装编排绑定到开始时的宿主与权威查询代次。 */
interface SkinHostAuthorityToken {
  epoch: number;
  host: SkinHostKind;
}

/** 识别后端在最后一刻发现 WorkBuddy 仍运行但无可用 CDP 的稳定恢复请求。 */
function isWorkBuddyRecoveryRequired(cause: unknown): cause is SkinHostError {
  return (
    cause instanceof SkinHostError && cause.code === "skin.workbuddy_recovery_required"
  );
}

/** 编排安装、启动、实例重启与记忆恢复，所有破坏性路径先产出确认状态。 */
export function useSkinHostActions({
  activeHost,
  host,
  hostStateReady,
  instanceList,
  refreshHost,
  setNotice,
}: SkinHostActionsOptions) {
  const { t } = useTranslation();
  const queryClient = useQueryClient();
  const setSession = useSetAtom(skinPageSessionAtom);
  const [appearance, setAppearance] = useState<SkinAppearanceRequest | null>(null);
  const [restartRequest, setRestartRequest] = useState<PendingHostRestart | null>(null);
  const [remembered, setRemembered] = useState<SkinReference | null>(null);
  const activeHostRef = useRef(activeHost);
  const authorityRef = useRef({ activeHost, epoch: 0, ready: hostStateReady });
  if (
    authorityRef.current.activeHost !== activeHost ||
    authorityRef.current.ready !== hostStateReady
  ) {
    authorityRef.current = {
      activeHost,
      epoch: authorityRef.current.epoch + 1,
      ready: hostStateReady,
    };
  }
  const selectedInstance = useMemo(
    () => resolveSoleTargetInstance(instanceList),
    [instanceList],
  );

  useEffect(() => {
    activeHostRef.current = activeHost;
    setRemembered(activeHost === null ? null : readRememberedSkin(activeHost));
  }, [activeHost]);

  /** 捕获当前可写代次；失权、恢复或切换宿主都会让旧操作失效。 */
  const captureAuthority = useCallback(
    (operationHost: SkinHostKind): SkinHostAuthorityToken => {
      const authority = authorityRef.current;
      if (!authority.ready || authority.activeHost !== operationHost) {
        throw new SkinHostError("skin.host_state_unavailable", t("skins.error.unknown"));
      }
      return { epoch: authority.epoch, host: operationHost };
    },
    [t],
  );

  /** 每个异步读取后、每个新的宿主副作用前复核同一权威代次。 */
  const assertAuthority = useCallback(
    (token: SkinHostAuthorityToken): void => {
      const authority = authorityRef.current;
      if (
        !authority.ready ||
        authority.activeHost !== token.host ||
        authority.epoch !== token.epoch
      ) {
        throw new SkinHostError("skin.host_state_unavailable", t("skins.error.unknown"));
      }
    },
    [t],
  );

  /** 在实例明确且无需重启时执行皮肤安装。 */
  const installOnInstance = useCallback(
    async (
      operationHost: SkinHostKind,
      skin: SkinDescriptor,
      target: CodexInstance | null,
      allowThirdPartyCode: boolean,
      allowMismatch = false,
      allowWorkBuddyRecovery = false,
      authorityToken = captureAuthority(operationHost),
    ): Promise<void> => {
      assertAuthority(authorityToken);
      const result = await skinApi.install(
        operationHost,
        skinReference(skin),
        allowMismatch,
        target?.id ?? null,
        allowWorkBuddyRecovery,
        allowThirdPartyCode,
      );
      assertAuthority(authorityToken);
      if (result.type === "needsConfirmation") {
        setAppearance({
          allowThirdPartyCode,
          allowWorkBuddyRecovery,
          check: result.check,
          host: operationHost,
          skin,
          target,
        });
        return;
      }
      const reference = skinReference(skin);
      rememberSkin(operationHost, reference);
      if (activeHostRef.current === operationHost) setRemembered(reference);
      setSession((value) => ({
        ...value,
        restoreDismissedHosts: {
          ...value.restoreDismissedHosts,
          [operationHost]: false,
        },
      }));
      setNotice(t("skins.notice.applied", { name: skin.name }));
      await refreshHost(operationHost);
    },
    [assertAuthority, captureAuthority, refreshHost, setNotice, setSession, t],
  );

  /** 按原生平台能力选择 Windows 全量恢复或既有单实例重启确认。 */
  const requestRestartConfirmation = useCallback(
    async (
      operationHost: SkinHostKind,
      skin: SkinDescriptor,
      target: CodexInstance | null,
      allowThirdPartyCode: boolean,
      authorityToken = captureAuthority(operationHost),
    ): Promise<void> => {
      assertAuthority(authorityToken);
      const windowsWorkBuddyRecovery =
        operationHost === "workBuddy" && (await skinApi.supportsWindowsWorkBuddyRecovery());
      assertAuthority(authorityToken);
      if (!windowsWorkBuddyRecovery && target === null) {
        throw new SkinHostError(
          "skin.host_instance_selection_required",
          t("skins.error.choose_instance"),
        );
      }
      setRestartRequest({
        allowThirdPartyCode,
        host: operationHost,
        instanceId: target?.id ?? null,
        instanceLabel: target?.label ?? "",
        mode: windowsWorkBuddyRecovery ? "recoverWindowsWorkBuddy" : "restartSelected",
        skin,
      });
    },
    [assertAuthority, captureAuthority, t],
  );

  /** 解析唯一目标并在可能影响宿主会话时先进入确认弹窗。 */
  const requestInstall = useCallback(
    async (
      skin: SkinDescriptor,
      allowThirdPartyCode: boolean,
      operationHost: SkinHostKind = host,
    ): Promise<void> => {
      const authorityToken = captureAuthority(operationHost);
      try {
        let current =
          operationHost === host
            ? instanceList
            : await queryClient.fetchQuery({
                queryFn: () => skinApi.instances(operationHost),
                queryKey: skinInstancesQueryKey(operationHost),
              });
        assertAuthority(authorityToken);
        if (current.length === 0) {
          assertAuthority(authorityToken);
          await skinApi.launchHost(operationHost);
          assertAuthority(authorityToken);
          current = await queryClient.fetchQuery({
            queryFn: () => skinApi.instances(operationHost),
            queryKey: skinInstancesQueryKey(operationHost),
          });
          assertAuthority(authorityToken);
        }
        const target = resolveSoleTargetInstance(current);
        if (target === null) {
          if (operationHost === "workBuddy" && needsWorkBuddyCdpRecovery(current)) {
            await requestRestartConfirmation(
              operationHost,
              skin,
              null,
              allowThirdPartyCode,
              authorityToken,
            );
            return;
          }
          throw new SkinHostError(
            "skin.host_instance_selection_required",
            t("skins.error.choose_instance"),
          );
        }
        if (target.state === "runningWithoutCdp" && operationHost !== "workBuddy") {
          await requestRestartConfirmation(
            operationHost,
            skin,
            target,
            allowThirdPartyCode,
            authorityToken,
          );
          return;
        }
        await installOnInstance(
          operationHost,
          skin,
          target,
          allowThirdPartyCode,
          false,
          false,
          authorityToken,
        );
      } catch (cause) {
        if (operationHost !== "workBuddy" || !isWorkBuddyRecoveryRequired(cause)) {
          throw cause;
        }
        assertAuthority(authorityToken);
        const refreshed = await queryClient.fetchQuery({
          queryFn: () => skinApi.instances(operationHost),
          queryKey: skinInstancesQueryKey(operationHost),
          staleTime: 0,
        });
        assertAuthority(authorityToken);
        const target = resolveSoleTargetInstance(refreshed);
        if (
          target === null &&
          refreshed.length > 1 &&
          !needsWorkBuddyCdpRecovery(refreshed)
        ) {
          throw new SkinHostError(
            "skin.host_instance_selection_required",
            t("skins.error.choose_instance"),
          );
        }
        await requestRestartConfirmation(
          operationHost,
          skin,
          target,
          allowThirdPartyCode,
          authorityToken,
        );
      }
    },
    [
      host,
      assertAuthority,
      captureAuthority,
      instanceList,
      installOnInstance,
      queryClient,
      requestRestartConfirmation,
      t,
    ],
  );

  /** 重启所选实例后仍只允许同一权威代次继续安装。 */
  const restartAndInstall = useCallback(
    async (
      operationHost: SkinHostKind,
      skin: SkinDescriptor,
      instanceId: string,
      allowThirdPartyCode: boolean,
    ): Promise<void> => {
      const authorityToken = captureAuthority(operationHost);
      const restarted = await skinApi.restartInstance(operationHost, instanceId);
      assertAuthority(authorityToken);
      await installOnInstance(
        operationHost,
        skin,
        restarted,
        allowThirdPartyCode,
        false,
        false,
        authorityToken,
      );
    },
    [assertAuthority, captureAuthority, installOnInstance],
  );

  return {
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
  };
}
