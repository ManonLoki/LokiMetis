import { Button, Group, Paper, Table, Text, UnstyledButton } from '@mantine/core';
import { useTranslation } from 'react-i18next';

import type {
  LocalIndexState,
  UsageCallItemDto,
  UsageCallSortField,
  UsageViewKind,
} from '../api/usage';
import { TokenTotalDisplay } from '../components/UsageUi';
import { displayLabel } from '../i18n/backend-labels';
import { agentClientLabel } from '../state/agent-client';
import type { UsageCallSort } from '../state/usage-filters';
import { formatObservedAt, formatTokens } from '../usage-format';

/** 渲染一个可访问的固定排序表头；稳定 ID 兜底由后端实现。 */
function SortHeader({
  align = 'left',
  field,
  label,
  onChange,
  sort,
}: {
  align?: 'left' | 'right';
  field: UsageCallSortField;
  label: string;
  onChange: (sort: UsageCallSort) => void;
  sort: UsageCallSort;
}) {
  const active = sort.field === field;
  return (
    <Table.Th
      aria-sort={active ? (sort.direction === 'asc' ? 'ascending' : 'descending') : undefined}
      ta={align}
    >
      <UnstyledButton
        className="sort-header"
        onClick={() =>
          onChange({
            direction: active && sort.direction === 'desc' ? 'asc' : 'desc',
            field,
          })
        }
        style={{ justifyContent: align === 'right' ? 'flex-end' : 'space-between' }}
      >
        <span>{label}</span>
        <span aria-hidden="true">{active ? (sort.direction === 'asc' ? '↑' : '↓') : '↕'}</span>
      </UnstyledButton>
    </Table.Th>
  );
}

/** 定义调用结果表的分页数据、排序状态与加载交互。 */
interface CallsResultsTableProps {
  /** 当前联合或物理视图。 */
  view: UsageViewKind;
  /** 当前稳定结果行。 */
  items: UsageCallItemDto[];
  /** 当前联合索引四态。 */
  indexState: LocalIndexState;
  /** 已应用的全局排序。 */
  sort: UsageCallSort;
  /** 筛选结果总数。 */
  totalCount: number;
  /** 当前是否在替换首屏。 */
  refreshingFirstPage: boolean;
  /** 当前首屏替换是否失败。 */
  replacementFailed: boolean;
  /** 是否仍有后页。 */
  hasNextPage: boolean;
  /** 是否正在加载后页。 */
  isFetchingNextPage: boolean;
  /** 切换排序并重建页链。 */
  onSortChange: (sort: UsageCallSort) => void;
  /** 加载后端给出的下一页。 */
  onFetchNextPage: () => void;
}

/** 展示后端已经全局筛选、排序和分页的调用结果，不在前端二次合并。 */
export function CallsResultsTable({
  hasNextPage,
  indexState,
  isFetchingNextPage,
  items,
  onFetchNextPage,
  onSortChange,
  refreshingFirstPage,
  replacementFailed,
  sort,
  totalCount,
  view,
}: CallsResultsTableProps) {
  const { t } = useTranslation();
  const columnCount = view === 'all' ? 13 : 12;
  return (
    <Paper className="table-panel calls-panel" radius="lg" withBorder>
      <Table.ScrollContainer
        aria-busy={refreshingFirstPage}
        aria-label={t('calls.table.aria')}
        minWidth={view === 'all' ? 1560 : 1440}
        tabIndex={0}
        type="native"
      >
        <Table highlightOnHover verticalSpacing="md">
          <Table.Thead>
            <Table.Tr>
              {view === 'all' ? <Table.Th ta="left">{t('calls.table.agent')}</Table.Th> : null}
              <SortHeader
                field="occurredAt"
                label={t('calls.table.time')}
                onChange={onSortChange}
                sort={sort}
              />
              <Table.Th ta="left">{t('dimension.project')}</Table.Th>
              <Table.Th ta="left">{t('dimension.thread')}</Table.Th>
              <SortHeader
                field="model"
                label={t('dimension.model')}
                onChange={onSortChange}
                sort={sort}
              />
              <SortHeader
                field="reasoningEffort"
                label={t('dimension.reasoningEffort')}
                onChange={onSortChange}
                sort={sort}
              />
              <SortHeader
                align="right"
                field="inputTokens"
                label={t('calls.table.inputTokens')}
                onChange={onSortChange}
                sort={sort}
              />
              <SortHeader
                align="right"
                field="totalTokens"
                label={t('metric.totalTokens')}
                onChange={onSortChange}
                sort={sort}
              />
              <SortHeader
                align="right"
                field="cachedInputTokens"
                label={t('metric.cachedInput')}
                onChange={onSortChange}
                sort={sort}
              />
              <Table.Th ta="right">{t('metric.cacheWrite')}</Table.Th>
              <SortHeader
                align="right"
                field="uncachedInputTokens"
                label={t('metric.uncachedInput')}
                onChange={onSortChange}
                sort={sort}
              />
              <SortHeader
                align="right"
                field="outputTokens"
                label={t('metric.output')}
                onChange={onSortChange}
                sort={sort}
              />
              <SortHeader
                align="right"
                field="reasoningOutputTokens"
                label={t('metric.reasoningOutput')}
                onChange={onSortChange}
                sort={sort}
              />
            </Table.Tr>
          </Table.Thead>
          <Table.Tbody>
            {refreshingFirstPage ? (
              <Table.Tr>
                <Table.Td colSpan={columnCount}>
                  <Text c="dimmed" py="xl" ta="center">
                    {t('calls.table.updating')}
                  </Text>
                </Table.Td>
              </Table.Tr>
            ) : replacementFailed ? (
              <Table.Tr>
                <Table.Td colSpan={columnCount}>
                  <Text c="red" py="xl" ta="center">
                    {t('calls.table.replacementFailed')}
                  </Text>
                </Table.Td>
              </Table.Tr>
            ) : (
              items.map((item) => (
                <Table.Tr key={item.id}>
                  {view === 'all' ? (
                    <Table.Td ta="left">{agentClientLabel(item.client)}</Table.Td>
                  ) : null}
                  <Table.Td ta="left">{formatObservedAt(item.occurredAtEpochMs)}</Table.Td>
                  <Table.Td ta="left">
                    {displayLabel(t, item.projectLabel, item.projectLabelCode)}
                  </Table.Td>
                  <Table.Td ta="left">
                    {displayLabel(t, item.threadLabel, item.threadLabelCode)}
                  </Table.Td>
                  <Table.Td ta="left">
                    {displayLabel(t, item.modelLabel, item.modelLabelCode)}
                  </Table.Td>
                  <Table.Td ta="left">
                    {item.client !== 'grokBuildCli'
                      ? displayLabel(t, item.reasoningEffortLabel, item.reasoningEffortLabelCode)
                      : t('common.notProvided')}
                  </Table.Td>
                  <Table.Td ta="right">
                    <TokenTotalDisplay density="inline" value={item.fact.value.inputTokens} />
                  </Table.Td>
                  <Table.Td fw={700} ta="right">
                    <TokenTotalDisplay density="inline" value={item.fact.value.totalTokens} />
                  </Table.Td>
                  <Table.Td ta="right">
                    <TokenTotalDisplay density="inline" value={item.fact.value.cachedInputTokens} />
                  </Table.Td>
                  <Table.Td ta="right">
                    <TokenTotalDisplay
                      density="inline"
                      value={item.fact.value.cacheWriteInputTokens}
                    />
                  </Table.Td>
                  <Table.Td ta="right">
                    <TokenTotalDisplay density="inline" value={item.uncachedInputTokens} />
                  </Table.Td>
                  <Table.Td ta="right">
                    <TokenTotalDisplay density="inline" value={item.fact.value.outputTokens} />
                  </Table.Td>
                  <Table.Td ta="right">
                    <TokenTotalDisplay
                      density="inline"
                      value={item.fact.value.reasoningOutputTokens}
                    />
                  </Table.Td>
                </Table.Tr>
              ))
            )}
          </Table.Tbody>
        </Table>
      </Table.ScrollContainer>
      {!refreshingFirstPage && !replacementFailed && items.length === 0 ? (
        <Text c="dimmed" p="xl" ta="center">
          {indexState === 'readyNoCalls'
            ? t('calls.table.emptyScanned')
            : t('calls.table.emptyFiltered')}
        </Text>
      ) : null}
      <Group className="calls-footer" justify="space-between" p="md">
        <Text c="dimmed" size="sm">
          {refreshingFirstPage
            ? t('calls.footer.readingFirstPage')
            : replacementFailed
              ? t('calls.footer.firstPageFailed')
              : t('calls.footer.loaded', {
                  loaded: formatTokens(items.length),
                  total: formatTokens(totalCount),
                })}
        </Text>
        {refreshingFirstPage ? (
          <Text c="dimmed" size="sm">
            {t('calls.footer.controlsAvailable')}
          </Text>
        ) : replacementFailed ? (
          <Text c="dimmed" size="sm">
            {t('calls.footer.retryAbove')}
          </Text>
        ) : hasNextPage ? (
          <Button loading={isFetchingNextPage} onClick={onFetchNextPage} variant="light">
            {t('calls.footer.loadMore')}
          </Button>
        ) : (
          <Text c="dimmed" size="sm">
            {t('calls.footer.allLoaded')}
          </Text>
        )}
      </Group>
    </Paper>
  );
}
