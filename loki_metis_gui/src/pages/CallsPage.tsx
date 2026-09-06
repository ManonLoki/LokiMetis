import {
  Alert,
  Button,
  Group,
  NativeSelect,
  Paper,
  SimpleGrid,
  Stack,
  Text,
} from "@mantine/core";
import { useInfiniteQuery, useQuery, type InfiniteData } from "@tanstack/react-query";
import { useAtom, useAtomValue } from "jotai";
import { useState } from "react";
import { useTranslation } from "react-i18next";
import type { TFunction } from "i18next";

import {
  getUsageCalls,
  getUsageOverview,
  timeStandardQueryKey,
  type UsageCallFiltersDto,
  type UsageCallsPageDto,
  type UsageFilterOptionDto,
  type UsageViewKind,
} from "../api/usage";
import {
  FailureState,
  ImplementationState,
  LocalIndexNotice,
  LoadingState,
} from "../components/UsageUi";
import {
  emptyUsageFilters,
  usageFiltersAtom,
  usageSortAtom,
  type UsageCallSort,
} from "../state/usage-filters";
import { usageViewAtom } from "../state/agent-client";
import { timeStandardAtom } from "../state/page-session";
import { visibleErrorMessage } from "../visible-error";
import { displayLabel } from "../i18n/backend-labels";
import { CallsResultsTable } from "./CallsResultsTable";

/** 把后端选项转换为 NativeSelect 数据，首项始终表示全部。 */
function filterOptions(
  t: TFunction,
  values: UsageFilterOptionDto[],
): { label: string; value: string }[] {
  return [
    { label: t("calls.filters.all"), value: "" },
    ...values.map(({ disambiguationIndex, id, label, labelCode }) => ({
      label: displayLabel(t, label, labelCode, disambiguationIndex),
      value: id,
    })),
  ];
}

/** 把选择器空字符串映射为后端稳定空筛选。 */
function updateFilter(
  filters: UsageCallFiltersDto,
  key: keyof UsageCallFiltersDto,
  value: string,
): UsageCallFiltersDto {
  return { ...filters, [key]: value || null };
}

/** 独立浏览全部启用根中去重后的调用，使用固定后端页大小逐页追加。 */
export function CallsPage() {
  const view = useAtomValue(usageViewAtom);
  return <LocalCallsPage view={view} />;
}

/** 为一个物理 Agent 或全部只读联合视图装配调用查询。 */
function LocalCallsPage({ view }: { view: UsageViewKind }) {
  const { t } = useTranslation();
  const [filters, setFilters] = useAtom(usageFiltersAtom);
  const [sort, setSort] = useAtom(usageSortAtom);
  const timeStandard = useAtomValue(timeStandardAtom);
  const [retainedFirstPage, setRetainedFirstPage] = useState<{
    view: UsageViewKind;
    page: UsageCallsPageDto;
  } | null>(null);
  const retainedForView = retainedFirstPage?.view === view ? retainedFirstPage.page : null;
  const reasoningAvailable = view !== "grokBuildCli";
  const effectiveFilters = reasoningAvailable
    ? filters
    : { ...filters, reasoningEffort: null };
  const overviewQuery = useQuery({
    queryFn: () => getUsageOverview(view, timeStandard),
    queryKey: ["usage-overview", view, ...timeStandardQueryKey(timeStandard)],
  });
  const businessReady =
    overviewQuery.isSuccess && !overviewQuery.data.productDefinitionRequired;
  // useInfiniteQuery：TanStack Query 专门用于“分页加载、后页追加到前页
  // 后面”场景的 hook（对应后端 calls_view.rs 里游标分页的 GET /calls）。
  // 泛型参数依次是：单页数据类型、错误类型、聚合后的数据结构、queryKey
  // 类型、分页参数（这里是 `string | null` 游标）类型。
  const callsQuery = useInfiniteQuery<
    UsageCallsPageDto,
    Error,
    InfiniteData<UsageCallsPageDto>,
    readonly unknown[],
    string | null
  >({
    enabled: businessReady,
    // getNextPageParam：从最后一页的响应里取出后端返回的不透明游标
    // （lastPage.nextCursor，对应 Rust calls_view.rs 里 encode_cursor
    // 生成的字符串），作为下一次加载更多时传给 queryFn 的 pageParam；
    // 没有更多数据时后端返回 null，这里转成 undefined 表示"到底了"。
    getNextPageParam: (lastPage) => lastPage.nextCursor ?? undefined,
    initialPageParam: null as string | null,
    // placeholderData 在切换客户端时刻意返回 undefined（不复用旧数据）：
    // 只有 queryKey 里的 client 字段跟当前一致才继续展示上一次的数据，
    // 避免切换 Codex/Claude 客户端时界面短暂闪现另一个客户端的旧调用记录。
    placeholderData: (previousData, previousQuery) =>
      previousQuery?.queryKey[1] === view ? previousData : undefined,
    queryFn: ({ pageParam }) =>
      getUsageCalls(
        view,
        {
          cursor: pageParam,
          filters: effectiveFilters,
          sortDirection: sort.direction,
          sortField: sort.field,
        },
        timeStandard,
      ),
    // queryKey 包含 filters 和 sort：这两者任一变化，TanStack Query 就会
    // 认为这是一个全新的查询（而不是"追加更多页"），自动从第一页重新开始，
    // 与后端游标里编码的"查询指纹变了就拒绝旧游标"是同一条规则在前端的体现。
    queryKey: [
      "usage-calls",
      view,
      effectiveFilters,
      sort,
      ...timeStandardQueryKey(timeStandard),
    ],
  });
  const currentFirstPage = callsQuery.data?.pages[0];

  // retainedFirstPage：筛选/排序切换的瞬间，新查询链还没有任何数据
  // （currentFirstPage 变成 undefined），但产品要求“筛选或排序刷新时也
  // 不会卸载页面与焦点”——所以在真正切换前，先把当前已知的首屏数据存进
  // 这个额外的 state，新查询完成前用它继续撑住表格结构和已有内容，
  // 只是背景显示“正在刷新”，而不是让整个表格瞬间清空重新出现。
  /** 在建立新查询链前保留安全的首屏元数据，失败时仍可维持页面结构。 */
  function retainCurrentShell() {
    if (currentFirstPage) {
      setRetainedFirstPage({ page: currentFirstPage, view });
    }
  }

  /** 从当前首屏开始新的筛选查询链。 */
  function changeFilter(key: keyof UsageCallFiltersDto, value: string) {
    retainCurrentShell();
    setFilters(updateFilter(filters, key, value));
  }

  /** 从当前首屏开始新的排序查询链。 */
  function changeSort(nextSort: UsageCallSort) {
    retainCurrentShell();
    setSort(nextSort);
  }

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
  if (callsQuery.isPending && !callsQuery.data && !retainedForView) {
    return <LoadingState label={t("calls.page.loading")} />;
  }
  if (callsQuery.isError && !callsQuery.data && !retainedForView) {
    return (
      <FailureState error={callsQuery.error} onRetry={() => void callsQuery.refetch()} />
    );
  }

  const pages = callsQuery.data?.pages ?? [];
  const firstPage = currentFirstPage ?? retainedForView;
  if (!firstPage) {
    return (
      <FailureState
        error={new Error(t("calls.page.missingFirstPage"))}
        fallback={t("calls.page.missingFirstPage")}
        onRetry={() => void callsQuery.refetch()}
      />
    );
  }
  const replacementFailed = callsQuery.isError && !callsQuery.data;
  const items = currentFirstPage ? pages.flatMap((page) => page.items) : [];
  const totalCount = currentFirstPage
    ? (pages.at(-1)?.totalCount ?? firstPage.totalCount)
    : 0;
  const options = firstPage.availableFilters;
  const refreshingFirstPage = callsQuery.isPlaceholderData && callsQuery.isFetching;
  const refreshFailed =
    replacementFailed || (callsQuery.isRefetchError && !callsQuery.isFetchNextPageError);

  return (
    <Stack className="page-stack" gap="xl">
      <LocalIndexNotice state={firstPage.indexState} />

      {firstPage.indexState === "notScanned" ||
      firstPage.indexState === "needsRescan" ? null : (
        <>
          <Paper className="filter-panel" p="lg" radius="lg" withBorder>
            <Stack gap="md">
              <Group justify="space-between">
                <div>
                  <Text fw={700}>{t("calls.filters.title")}</Text>
                  <Text c="dimmed" size="sm">
                    {t("calls.filters.description")}
                  </Text>
                </div>
                <Button
                  onClick={() => {
                    retainCurrentShell();
                    setFilters(emptyUsageFilters);
                  }}
                  variant="subtle"
                >
                  {t("calls.filters.clear")}
                </Button>
              </Group>
              <SimpleGrid cols={{ base: 1, sm: 2, xl: 5 }}>
                <NativeSelect
                  data={filterOptions(t, options.models)}
                  label={t("dimension.model")}
                  onChange={(event) => changeFilter("model", event.currentTarget.value)}
                  value={filters.model || ""}
                />
                {reasoningAvailable ? (
                  <NativeSelect
                    data={filterOptions(t, options.reasoningEfforts)}
                    label={t("dimension.reasoningEffort")}
                    onChange={(event) =>
                      changeFilter("reasoningEffort", event.currentTarget.value)
                    }
                    value={filters.reasoningEffort || ""}
                  />
                ) : null}
                <NativeSelect
                  data={filterOptions(t, options.projects)}
                  label={t("dimension.project")}
                  onChange={(event) => changeFilter("project", event.currentTarget.value)}
                  value={filters.project || ""}
                />
                <NativeSelect
                  data={filterOptions(t, options.threads)}
                  label={t("dimension.thread")}
                  onChange={(event) => changeFilter("thread", event.currentTarget.value)}
                  value={filters.thread || ""}
                />
                <NativeSelect
                  data={filterOptions(t, options.roots)}
                  label={t("dimension.root")}
                  onChange={(event) => changeFilter("root", event.currentTarget.value)}
                  value={filters.root || ""}
                />
              </SimpleGrid>
            </Stack>
          </Paper>

          {refreshFailed ? (
            <Alert color="red" title={t("calls.refresh.title")}>
              <Group justify="space-between">
                <Text size="sm">
                  {visibleErrorMessage(callsQuery.error, t("calls.refresh.retained"))}
                </Text>
                <Button onClick={() => void callsQuery.refetch()} size="xs" variant="light">
                  {t("calls.refresh.retry")}
                </Button>
              </Group>
            </Alert>
          ) : null}

          <CallsResultsTable
            hasNextPage={callsQuery.hasNextPage}
            indexState={firstPage.indexState}
            isFetchingNextPage={callsQuery.isFetchingNextPage}
            items={items}
            onFetchNextPage={() => void callsQuery.fetchNextPage()}
            onSortChange={changeSort}
            refreshingFirstPage={refreshingFirstPage}
            replacementFailed={replacementFailed}
            sort={sort}
            totalCount={totalCount}
            view={view}
          />

          {callsQuery.isFetchNextPageError ? (
            <Alert color="red" title={t("calls.nextPage.title")}>
              <Group justify="space-between">
                <Text size="sm">
                  {visibleErrorMessage(callsQuery.error, t("calls.nextPage.retained"))}
                </Text>
                <Button
                  onClick={() => void callsQuery.fetchNextPage()}
                  size="xs"
                  variant="light"
                >
                  {t("calls.nextPage.retry")}
                </Button>
              </Group>
            </Alert>
          ) : null}
        </>
      )}
    </Stack>
  );
}
