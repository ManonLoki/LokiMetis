import { Stack } from "@mantine/core";
import { keepPreviousData, useQuery } from "@tanstack/react-query";
import { useAtom, useAtomValue } from "jotai";
import { useTranslation } from "react-i18next";

import {
  getUsageOverview,
  getUsageStatistics,
  timeStandardQueryKey,
  type AgentClientKind,
} from "../api/usage";
import {
  FailureState,
  ImplementationState,
  LocalIndexNotice,
  LoadingState,
} from "../components/UsageUi";
import { UsageStatistics } from "./UsageStatistics";
import { agentClientAtom, usageViewAtom } from "../state/agent-client";
import {
  timeStandardAtom,
  usageDimensionAtom,
  usageWindowAtom,
} from "../state/page-session";
import { WorkbuddyUsage } from "./WorkbuddyUsage";

/** 统计快照每次读取完成后等待十秒再刷新，慢请求期间保持单飞。 */
const STATISTICS_REFRESH_INTERVAL_MS = 10_000;

/** 独立展示本机趋势和固定维度分组；逐条调用由“调用”页面负责。 */
export function UsagePage() {
  const view = useAtomValue(usageViewAtom);
  const client = useAtomValue(agentClientAtom);
  if (view === "workbuddy") {
    return <WorkbuddyUsage />;
  }
  return <LocalUsagePage client={client} />;
}

/** 只为拥有本机索引能力的客户端装配统计查询，并保留当前进程内查询条件。 */
function LocalUsagePage({ client }: { client: AgentClientKind }) {
  const { t } = useTranslation();
  const [statisticsWindow, setStatisticsWindow] = useAtom(usageWindowAtom);
  const [selectedDimension, setSelectedDimension] = useAtom(usageDimensionAtom);
  const timeStandard = useAtomValue(timeStandardAtom);
  // reasoningEffort 分组只对实际发出推理强度的客户端有意义。Grok 若读到
  // 不可用的推理强度，只把展示与查询回退为 model，不得改写本客户端已存值，
  // 更不得覆盖 Codex 已保存的维度。
  const reasoningAvailable = client === "codex" || client === "claudeCode";
  const effectiveDimension =
    !reasoningAvailable && selectedDimension === "reasoningEffort"
      ? "model"
      : selectedDimension;
  const overviewQuery = useQuery({
    queryFn: () => getUsageOverview(client, timeStandard),
    queryKey: ["usage-overview", client, ...timeStandardQueryKey(timeStandard)],
  });
  const businessReady =
    overviewQuery.isSuccess && !overviewQuery.data.productDefinitionRequired;
  const statisticsQuery = useQuery({
    enabled: businessReady,
    // 切换统计窗口、分组维度或查看标准时查询键跟着变，没有这个选项会先丢弃已有数据、
    // 把整块面板（含选择器本身）换成通用加载态，看起来像“选不动”；
    // 保留上一次数据当占位符，配合下面已有的 fetching 徽标平滑过渡。
    placeholderData: keepPreviousData,
    queryFn: () =>
      getUsageStatistics(client, statisticsWindow, effectiveDimension, timeStandard),
    queryKey: [
      "usage-statistics",
      client,
      statisticsWindow,
      effectiveDimension,
      ...timeStandardQueryKey(timeStandard),
    ],
    // “读完成后等待 10 秒再刷新”而不是固定 10 秒间隔轮询：如果上一次
    // 请求还在进行中（fetchStatus === 'fetching'），这一轮先返回 false
    // 跳过，避免请求排队堆积；等它结束后才重新进入正常的 10 秒倒计时，
    // 这就是“单飞”（single-flight）——任意时刻最多只有一个进行中的请求。
    refetchInterval: (query) =>
      query.state.fetchStatus === "fetching" ? false : STATISTICS_REFRESH_INTERVAL_MS,
    refetchIntervalInBackground: false,
  });

  if (overviewQuery.isPending) {
    return <LoadingState />;
  }
  if (overviewQuery.isError) {
    return (
      <FailureState
        error={overviewQuery.error}
        onRetry={() => void overviewQuery.refetch()}
      />
    );
  }
  if (overviewQuery.data.productDefinitionRequired) {
    return (
      <ImplementationState
        message={overviewQuery.data.implementationMessage}
        messageCode={overviewQuery.data.implementationMessageCode}
      />
    );
  }
  if (statisticsQuery.isPending) {
    return <LoadingState label={t("statistics.page.loading")} />;
  }
  if (statisticsQuery.isError) {
    return (
      <FailureState
        error={statisticsQuery.error}
        onRetry={() => void statisticsQuery.refetch()}
      />
    );
  }

  return (
    <Stack className="page-stack" gap="xl">
      <LocalIndexNotice state={statisticsQuery.data.indexState} />

      <UsageStatistics
        dimension={effectiveDimension}
        fetching={statisticsQuery.isFetching}
        onDimensionChange={setSelectedDimension}
        onWindowChange={setStatisticsWindow}
        statistics={statisticsQuery.data}
        reasoningAvailable={reasoningAvailable}
        timeStandard={timeStandard}
        window={statisticsWindow}
      />
    </Stack>
  );
}
