import { useQuery, useQueryClient, type QueryClient } from '@tanstack/react-query';
import { useEffect, useRef, useState } from 'react';
import { useAtomValue } from 'jotai';

import {
  getLocalScanStatus,
  type AgentClientKind,
  type PrivacySettingsDto,
  type ScanStatusDto,
} from './usage';
import { agentClientAtom } from '../state/agent-client';

/** 扫描进行中时的状态轮询间隔：足够快显得实时，又不会打满 Tauri IPC 桥。 */
export const SCAN_STATUS_POLL_INTERVAL_MS = 1_500;

// TanStack Query 的 queryKey 是一个数组（如 ['usage-overview', 'codex']），
// 它的缓存 API（cancelQueries/removeQueries/invalidateQueries）在只给出
// 前缀（如 ['usage-overview']）时，会匹配所有以这个前缀开头的完整
// key——不需要手动枚举每个客户端、每个筛选条件组合出来的具体 key，
// 这也是本文件反复出现“只传前缀数组”写法的原因。

/** 初始化门禁翻转时必须彻底丢弃的查询前缀；初始化状态本身不在此列表。 */
const initializationSensitiveQueryPrefixes = [
  'usage-overview',
  'usage-statistics',
  'usage-charts',
  'usage-calls',
  'usage-sources',
  'source-roots',
  'privacy-settings',
  'scan-status',
  'provider-leaderboard',
] as const;

/** 取消并移除两个客户端的全部业务缓存，避免重新初始化后首帧复用旧账号或旧主根。 */
export function clearUsageQueriesForInitialization(queryClient: QueryClient): void {
  for (const prefix of initializationSensitiveQueryPrefixes) {
    void queryClient.cancelQueries({ queryKey: [prefix] });
    queryClient.removeQueries({ queryKey: [prefix] });
  }
}

/** 让所有依赖本机索引的视图在扫描、移除或清空后读取同一代数据。 */
export async function invalidateLocalUsageQueries(
  queryClient: QueryClient,
  client?: AgentClientKind,
): Promise<void> {
  const suffix = client ? [client] : [];
  const invalidations: Promise<unknown>[] = [
    queryClient.invalidateQueries({ queryKey: ['usage-overview', ...suffix] }),
    queryClient.invalidateQueries({ queryKey: ['usage-statistics', ...suffix] }),
    queryClient.invalidateQueries({ queryKey: ['usage-charts', ...suffix] }),
    queryClient.resetQueries({ queryKey: ['usage-calls', ...suffix] }),
    queryClient.invalidateQueries({ queryKey: ['usage-sources', ...suffix] }),
    queryClient.invalidateQueries({ queryKey: ['source-roots', ...suffix] }),
    queryClient.invalidateQueries({ queryKey: ['privacy-settings', ...suffix] }),
  ];
  if (client) {
    invalidations.push(
      queryClient.invalidateQueries({ queryKey: ['usage-overview', 'all'] }),
      queryClient.invalidateQueries({ queryKey: ['usage-charts', 'all'] }),
      queryClient.resetQueries({ queryKey: ['usage-calls', 'all'] }),
    );
  }
  await Promise.all(invalidations);
}

/** 将 runtime 全局设置同步进所有客户端缓存，同时保留各自独立索引元数据。 */
// PrivacySettingsDto 里的字段其实混合了两种作用域：像语言偏好、设备用户名、
// 刷新间隔、已开放 Agent、WorkBuddy 开关这些是“全局唯一一份”的设置（不区分
// Codex/Claude），但索引位置、索引大小这些字段是“每个客户端各自独立”的。缓存却是按
// ['privacy-settings', client] 分客户端存的，所以改一次全局设置后，
// 必须把变化同步广播进两个客户端各自的缓存条目里。
// `setQueriesData`（复数）能一次性匹配前缀命中的全部 query（这里是
// 两个客户端各一条），对每条都跑同一个更新函数——只覆盖全局字段，
// 用展开运算符 `...existing` 保留每条缓存里客户端专属的字段不被覆盖。
// Privacy 页（语言/设备身份/扫描间隔）与数据源页（数据读取模式）都会
// 写入这些全局字段，因此这个广播助手在两个页面间共享，而不是各自
// 复制一份。
export function synchronizeGlobalPrivacySettings(
  queryClient: QueryClient,
  settings: PrivacySettingsDto,
): void {
  queryClient.setQueriesData<PrivacySettingsDto>({ queryKey: ['privacy-settings'] }, (existing) =>
    existing
      ? {
          ...existing,
          languagePreference: settings.languagePreference,
          deviceUsername: settings.deviceUsername,
          deviceName: settings.deviceName,
          deviceUniqueId: settings.deviceUniqueId,
          localOnly: settings.localOnly,
          scanIntervalMinutes: settings.scanIntervalMinutes,
          retentionDays: settings.retentionDays,
          deviceTimeZone: settings.deviceTimeZone,
          enabledAgents: settings.enabledAgents,
          workbuddyStatsEnabled: settings.workbuddyStatsEnabled,
        }
      : existing,
  );
}

/** 在扫描开始后跨页面持续观察终态，确保离开数据源页也不会留下旧统计缓存。 */
// 这个 hook 被挂在 App.tsx 的 AppQueryEffects 里，跨越全部路由存活
// （不属于任何具体页面），职责是：只要后台扫描（不管是用户点的还是
// 周期性自动触发的）在运行，就持续轮询扫描状态；一旦观察到它从
// running 变成某个终态（completed/failed/cancelled），立即让概览、
// 统计、调用等缓存失效，这样用户即使当时正停留在别的页面，
// 回到相关页面时看到的也是扫描后的最新数据，而不是扫描前的旧缓存。
//
// 三个 ref 各自的作用：
//   lastScanState  —— 记录上一次观察到的状态，用来判断“是不是刚从别的
//                      状态切换到 running”（即一次新扫描的开始）；
//   scanSequence   —— 每次识别到新一轮扫描开始就自增，用于下面
//                      terminalIdentity 的组成部分，避免不同轮次的
//                      终态被误判为同一次；
//   invalidatedTerminal —— 记录“已经为哪个具体终态做过缓存失效”，
//                      防止同一个终态因为多次渲染/事件触发而重复
//                      invalidate。
/** 订阅本机扫描终态并使所有相关用量查询在一次状态跃迁后失效。 */
export function useLocalUsageRefreshObserver(): void {
  const client = useAtomValue(agentClientAtom);
  const localClient = client;
  const queryClient = useQueryClient();
  const lastScanState = useRef<ScanStatusDto['state'] | null>(null);
  const scanSequence = useRef(0);
  const invalidatedTerminal = useRef<string | null>(null);
  // 首次挂载主动读取一次状态，才能观察 Rust setup 在页面渲染前启动的自动快速扫描。
  const [monitoring, setMonitoring] = useState(true);

  useEffect(() => {
    if (!localClient) {
      lastScanState.current = null;
      invalidatedTerminal.current = null;
      return;
    }
    let active = true;
    // Defer via microtask so a StrictMode double-invoke unmount (which flips `active`
    // to false in cleanup) can cancel this before it fires on a stale mount.
    queueMicrotask(() => {
      if (active) {
        setMonitoring(true);
      }
    });
    lastScanState.current = null;
    invalidatedTerminal.current = null;
    const observeScanState = () => {
      const scan = queryClient.getQueryData<ScanStatusDto>(['scan-status', localClient]);
      if (!scan) {
        return;
      }
      if (scan.state === 'running') {
        if (lastScanState.current !== 'running') {
          scanSequence.current += 1;
          invalidatedTerminal.current = null;
        }
        lastScanState.current = scan.state;
        setMonitoring(true);
        return;
      }
      if (scan.state === 'idle' || scan.finishedAtEpochMs === null) {
        lastScanState.current = scan.state;
        setMonitoring(false);
        return;
      }
      // 把足以唯一标识"这一次具体扫描结束事件"的多个字段拼成一个字符串
      // 当作去重键，比单独用任何一个字段都更可靠（scanId 在某些回退路径
      // 下可能相同或缺失，加上轮次序号和起止时间形成组合身份）。
      const terminalIdentity = [
        scanSequence.current,
        scan.scanId ?? 'unknown',
        scan.startedAtEpochMs ?? 'unknown',
        scan.finishedAtEpochMs,
        scan.state,
      ].join(':');
      lastScanState.current = scan.state;
      setMonitoring(false);
      if (invalidatedTerminal.current !== terminalIdentity) {
        invalidatedTerminal.current = terminalIdentity;
        void invalidateLocalUsageQueries(queryClient, localClient);
      }
    };
    observeScanState();
    // getQueryCache().subscribe：订阅整个 QueryClient 缓存的底层事件流，
    // 不局限于某一个 useQuery 调用；这里用它在“scan-status 缓存被写入
    // 新数据”时触发 observeScanState 重新判断，等价于把这个观察器变成
    // 一个跨组件、跨路由都持续生效的后台监听器。
    const unsubscribe = queryClient.getQueryCache().subscribe((event) => {
      if (event.query.queryKey[0] !== 'scan-status') {
        return;
      }
      // queryCache.subscribe() notifies synchronously mid-update; defer to a microtask
      // so observeScanState reads settled query state instead of a half-committed one.
      queueMicrotask(() => {
        if (active) {
          observeScanState();
        }
      });
    });
    return () => {
      active = false;
      unsubscribe();
    };
  }, [localClient, queryClient]);

  useQuery({
    enabled: monitoring && localClient !== null,
    queryFn: () => {
      return getLocalScanStatus(localClient);
    },
    queryKey: ['scan-status', client],
    // refetchInterval 可以是一个函数：只要扫描仍在运行就按固定间隔轮询，
    // 一旦不在运行返回 false 直接停止自动轮询（而不是持续空转请求）——
    // TanStack Query 会在每次数据更新后重新调用这个函数决定下一次何时轮询。
    refetchInterval: (query) =>
      query.state.data?.state === 'running' ? SCAN_STATUS_POLL_INTERVAL_MS : false,
  });
}
