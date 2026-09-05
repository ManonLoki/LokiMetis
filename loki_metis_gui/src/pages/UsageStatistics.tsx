import { Badge, Group, NativeSelect, Paper, SimpleGrid, Stack, Table, Text } from '@mantine/core';
import { useTranslation } from 'react-i18next';

import type { TimeStandard, UsageDimension, UsageStatisticsDto, UsageWindow } from '../api/usage';
import { TokenTotalDisplay } from '../components/UsageUi';
import {
  completenessLabel,
  confidenceLabel,
  formatBasisPoints,
  formatCalendarDate,
  formatObservedAt,
  formatTokens,
} from '../usage-format';
import { displayLabel } from '../i18n/backend-labels';
import { usageWindowOrder } from './overview-windows';

/** 描述统计展示与固定查询选择器之间的受控交互。 */
interface UsageStatisticsProps {
  /** 当前后端一致快照。 */
  statistics: UsageStatisticsDto;
  /** 当前统计窗口。 */
  window: UsageWindow;
  /** 当前固定分组维度。 */
  dimension: UsageDimension;
  /** 查询切换期间用于解释旧快照仍在可见。 */
  fetching: boolean;
  /** 只接收六个固定窗口的更新回调。 */
  onWindowChange: (window: UsageWindow) => void;
  /** 只接收五个固定维度的更新回调。 */
  onDimensionChange: (dimension: UsageDimension) => void;
  /** Grok 等未提供稳定推理强度的客户端隐藏该分组入口。 */
  reasoningAvailable: boolean;
  /** 已保存的查看时间标准，日桶与分组共用同一窗口。 */
  timeStandard: TimeStandard;
}

const dimensionValues: UsageDimension[] = ['model', 'reasoningEffort', 'project', 'thread', 'root'];

/** 展示同一 canonical 快照生成的摘要、逐日趋势与 Top-N 分组。 */
export function UsageStatistics({
  statistics,
  window,
  dimension,
  fetching,
  onWindowChange,
  onDimensionChange,
  reasoningAvailable,
  timeStandard,
}: UsageStatisticsProps) {
  const { t } = useTranslation();
  const aggregate = statistics.fact.value;
  // statistics.groups 是后端 core/src/statistics.rs 的 group_usage 算出的
  // Top-N 分组（默认前 10），statistics.remainder 是"其余项"合并后的
  // 一行聚合；这里只是把 remainder（如果存在）当作表格最后一行拼接
  // 展示，用户能看到"前 10 名 + 其余全部"两部分合计正好等于总数，
  // 不会因为截断丢失可核对性。
  const groupRows = statistics.remainder
    ? [...statistics.groups, statistics.remainder]
    : statistics.groups;
  const windowOptions = usageWindowOrder.map((value) => ({ label: t(`window.${value}`), value }));
  const dimensionOptions = dimensionValues.map((value) => ({
    label: t(`dimension.${value}`),
    value,
  }));

  return (
    <Stack gap="lg">
      <Paper
        aria-label={t('statistics.controls.aria')}
        className="filter-panel"
        p="lg"
        radius="lg"
        withBorder
      >
        <SimpleGrid cols={{ base: 1, sm: 2 }}>
          <NativeSelect
            data={windowOptions}
            label={t('statistics.controls.window')}
            onChange={(event) => onWindowChange(event.currentTarget.value as UsageWindow)}
            value={window}
          />
          <NativeSelect
            data={dimensionOptions.filter(
              (option) => reasoningAvailable || option.value !== 'reasoningEffort',
            )}
            label={t('statistics.controls.dimension')}
            onChange={(event) => onDimensionChange(event.currentTarget.value as UsageDimension)}
            value={dimension}
          />
        </SimpleGrid>
        <Group gap="xs" justify="space-between" mt="md">
          <Group gap="xs">
            <Badge color="yellow" variant="light">
              {t('statistics.controls.localRecords')}
            </Badge>
            <Badge
              color={statistics.fact.completeness === 'complete' ? 'blue' : 'orange'}
              variant="light"
            >
              {completenessLabel(statistics.fact.completeness)}
            </Badge>
            <Badge
              color={statistics.fact.confidence === 'exact' ? 'blue' : 'orange'}
              variant="light"
            >
              {confidenceLabel(statistics.fact.confidence)}
            </Badge>
            <Text c="dimmed" size="xs">
              {t('statistics.controls.observed', {
                date: formatObservedAt(statistics.observedAtEpochMs),
              })}
            </Text>
          </Group>
          <Badge color={fetching ? 'orange' : 'blue'} variant="light">
            {fetching ? t('statistics.controls.updating') : t('statistics.controls.sameSnapshot')}
          </Badge>
        </Group>
      </Paper>

      <SimpleGrid cols={{ base: 1, sm: 2, lg: 4 }}>
        <Paper className="mini-metric" p="lg" radius="lg" withBorder>
          <Text c="dimmed" size="sm">
            {t('statistics.summary.totalTokens')}
          </Text>
          <TokenTotalDisplay className="window-number" value={aggregate.tokens.totalTokens} />
        </Paper>
        <Paper className="mini-metric" p="lg" radius="lg" withBorder>
          <Text c="dimmed" size="sm">
            {t('statistics.summary.calls')}
          </Text>
          <Text className="window-number" fw={800}>
            {formatTokens(aggregate.callCount)}
          </Text>
        </Paper>
        <Paper className="mini-metric" p="lg" radius="lg" withBorder>
          <Text c="dimmed" size="sm">
            {t('statistics.summary.threads')}
          </Text>
          <Text className="window-number" fw={800}>
            {formatTokens(aggregate.threadCount)}
          </Text>
        </Paper>
        <Paper className="mini-metric" p="lg" radius="lg" withBorder>
          <Text c="dimmed" size="sm">
            {t('statistics.summary.cacheReadShare')}
          </Text>
          <Text className="window-number" fw={800}>
            {aggregate.tokens.cachedInputTokens === null
              ? t('common.notProvided')
              : formatBasisPoints(aggregate.cacheReadBasisPoints)}
          </Text>
        </Paper>
      </SimpleGrid>

      <Paper className="table-panel" radius="lg" withBorder>
        <Stack gap={0}>
          <Group justify="space-between" p="lg">
            <Text fw={700}>{t('statistics.daily.title')}</Text>
          </Group>
          <Table.ScrollContainer minWidth={760}>
            <Table verticalSpacing="sm">
              <Table.Thead>
                <Table.Tr>
                  <Table.Th>
                    {timeStandard.mode === 'custom'
                      ? t('statistics.daily.dateRemote')
                      : t('statistics.daily.dateLocal')}
                  </Table.Th>
                  <Table.Th ta="right">{t('metric.totalTokens')}</Table.Th>
                  <Table.Th ta="right">{t('metric.input')}</Table.Th>
                  <Table.Th ta="right">{t('metric.cachedInput')}</Table.Th>
                  <Table.Th ta="right">{t('metric.output')}</Table.Th>
                  <Table.Th ta="right">{t('metric.calls')}</Table.Th>
                </Table.Tr>
              </Table.Thead>
              <Table.Tbody>
                {statistics.dailyBuckets.map((bucket) => (
                  <Table.Tr key={bucket.localDate}>
                    <Table.Td>
                      <Group gap="xs" wrap="nowrap">
                        <Text>{formatCalendarDate(bucket.localDate)}</Text>
                        {bucket.inProgress ? (
                          <Badge color="orange" size="xs" variant="light">
                            {t('statistics.daily.inProgress')}
                          </Badge>
                        ) : null}
                      </Group>
                    </Table.Td>
                    <Table.Td fw={700} ta="right">
                      <TokenTotalDisplay
                        density="inline"
                        value={bucket.measure.tokens.totalTokens}
                      />
                    </Table.Td>
                    <Table.Td ta="right">
                      <TokenTotalDisplay
                        density="inline"
                        value={bucket.measure.tokens.inputTokens}
                      />
                    </Table.Td>
                    <Table.Td ta="right">
                      <TokenTotalDisplay
                        density="inline"
                        value={bucket.measure.tokens.cachedInputTokens}
                      />
                    </Table.Td>
                    <Table.Td ta="right">
                      <TokenTotalDisplay
                        density="inline"
                        value={bucket.measure.tokens.outputTokens}
                      />
                    </Table.Td>
                    <Table.Td ta="right">{formatTokens(bucket.measure.callCount)}</Table.Td>
                  </Table.Tr>
                ))}
              </Table.Tbody>
            </Table>
          </Table.ScrollContainer>
        </Stack>
      </Paper>

      <Paper className="table-panel" radius="lg" withBorder>
        <Stack gap={0}>
          <Group justify="space-between" p="lg">
            <Text fw={700}>{t('statistics.groups.title')}</Text>
          </Group>
          <Table.ScrollContainer minWidth={760}>
            <Table highlightOnHover verticalSpacing="sm">
              <Table.Thead>
                <Table.Tr>
                  <Table.Th>
                    {t(`dimension.${dimension}`, { defaultValue: t('dimension.group') })}
                  </Table.Th>
                  <Table.Th ta="right">{t('metric.totalTokens')}</Table.Th>
                  <Table.Th ta="right">{t('statistics.groups.tokenShare')}</Table.Th>
                  <Table.Th ta="right">{t('metric.calls')}</Table.Th>
                  <Table.Th ta="right">{t('metric.cacheReadShare')}</Table.Th>
                  <Table.Th>{t('statistics.groups.quality')}</Table.Th>
                </Table.Tr>
              </Table.Thead>
              <Table.Tbody>
                {groupRows.map((group) => (
                  <Table.Tr key={group.id}>
                    <Table.Td>
                      <Group gap="xs">
                        <Text fw={group.remainder ? 700 : 500}>
                          {displayLabel(t, group.label, group.labelCode, group.disambiguationIndex)}
                        </Text>
                        {group.remainder ? (
                          <Badge color="gray" size="xs" variant="light">
                            {t('statistics.groups.merged')}
                          </Badge>
                        ) : null}
                      </Group>
                    </Table.Td>
                    <Table.Td fw={700} ta="right">
                      <TokenTotalDisplay
                        density="inline"
                        value={group.measure.tokens.totalTokens}
                      />
                    </Table.Td>
                    <Table.Td ta="right">
                      {formatBasisPoints(group.totalTokenShareBasisPoints)}
                    </Table.Td>
                    <Table.Td ta="right">{formatTokens(group.measure.callCount)}</Table.Td>
                    <Table.Td ta="right">
                      {group.measure.tokens.cachedInputTokens === null
                        ? t('common.notProvided')
                        : formatBasisPoints(group.measure.cacheReadBasisPoints)}
                    </Table.Td>
                    <Table.Td>{confidenceLabel(group.measure.confidence)}</Table.Td>
                  </Table.Tr>
                ))}
              </Table.Tbody>
            </Table>
          </Table.ScrollContainer>
          {groupRows.length === 0 ? (
            <Text c="dimmed" p="xl" ta="center">
              {t('statistics.groups.empty')}
            </Text>
          ) : null}
        </Stack>
      </Paper>
    </Stack>
  );
}
