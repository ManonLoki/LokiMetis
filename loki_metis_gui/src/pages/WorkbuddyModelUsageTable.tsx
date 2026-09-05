import { Badge, Group, Paper, Stack, Table, Text } from '@mantine/core';
import { useTranslation } from 'react-i18next';

import type { WorkbuddyModelUsageWindowDto } from '../api/usage';
import { TokenTotalDisplay } from '../components/UsageUi';
import { formatCredits, formatTokens } from '../usage-format';

/** WorkBuddy project JSONL 实际执行模型的逐模型展示属性。 */
interface WorkbuddyModelUsageTableProps {
  /** 与通用统计使用同一批事件、观测时刻和查看时间标准的模型明细。 */
  modelUsage: WorkbuddyModelUsageWindowDto;
}

/**
 * 展示实际执行模型的请求、输入、缓存、输出和积分分项。
 * 所有行与上方统计均来自同一批 project JSONL usage 事件，因此可以直接对账。
 */
export function WorkbuddyModelUsageTable({ modelUsage }: WorkbuddyModelUsageTableProps) {
  const { t } = useTranslation();
  const groups = modelUsage.groups;

  return (
    <Paper
      aria-label={t('workbuddy.modelUsage.title')}
      className="table-panel"
      data-testid="workbuddy-model-usage"
      radius="lg"
      withBorder
    >
      <Stack gap={0}>
        <Stack gap="xs" p="lg">
          <Group justify="space-between">
            <Text fw={700}>{t('workbuddy.modelUsage.title')}</Text>
            <Badge color="violet" variant="light">
              {t('workbuddy.modelUsage.jsonlScope')}
            </Badge>
          </Group>
          <Text c="dimmed" size="sm">
            {t('workbuddy.modelUsage.description')}
          </Text>
        </Stack>

        <Table.ScrollContainer minWidth={1320}>
          <Table verticalSpacing="sm">
            <Table.Thead>
              <Table.Tr>
                <Table.Th>{t('workbuddy.modelUsage.model')}</Table.Th>
                <Table.Th ta="right">{t('metric.totalTokens')}</Table.Th>
                <Table.Th ta="right">{t('metric.input')}</Table.Th>
                <Table.Th ta="right">{t('metric.cachedInput')}</Table.Th>
                <Table.Th ta="right">{t('metric.uncachedInput')}</Table.Th>
                <Table.Th ta="right">{t('metric.output')}</Table.Th>
                <Table.Th ta="right">{t('workbuddy.modelUsage.requests')}</Table.Th>
                <Table.Th ta="right">{t('workbuddy.modelUsage.topLevelCalls')}</Table.Th>
                <Table.Th ta="right">{t('workbuddy.modelUsage.subagentCalls')}</Table.Th>
                <Table.Th ta="right">{t('workbuddy.totalCredits')}</Table.Th>
              </Table.Tr>
            </Table.Thead>
            <Table.Tbody>
              {groups.length === 0 ? (
                <Table.Tr>
                  <Table.Td colSpan={10}>
                    <Text c="dimmed" py="md" ta="center">
                      {t('workbuddy.modelUsage.empty')}
                    </Text>
                  </Table.Td>
                </Table.Tr>
              ) : (
                groups.map((group, index) => (
                  <Table.Tr key={group.model ?? `unattributed-${index}`}>
                    <Table.Td fw={700}>
                      {group.model ?? t('workbuddy.modelUsage.unattributed')}
                    </Table.Td>
                    <Table.Td ta="right">
                      <TokenTotalDisplay density="inline" value={group.totalTokens} />
                    </Table.Td>
                    <Table.Td ta="right">
                      <TokenTotalDisplay density="inline" value={group.inputTokens} />
                    </Table.Td>
                    <Table.Td ta="right">
                      <TokenTotalDisplay density="inline" value={group.cachedInputTokens} />
                    </Table.Td>
                    <Table.Td ta="right">
                      <TokenTotalDisplay density="inline" value={group.uncachedInputTokens} />
                    </Table.Td>
                    <Table.Td ta="right">
                      <TokenTotalDisplay density="inline" value={group.outputTokens} />
                    </Table.Td>
                    <Table.Td ta="right">{formatTokens(group.callCount)}</Table.Td>
                    <Table.Td ta="right">{formatTokens(group.topLevelCallCount)}</Table.Td>
                    <Table.Td ta="right">{formatTokens(group.subagentCallCount)}</Table.Td>
                    <Table.Td ta="right">{formatCredits(group.credits)}</Table.Td>
                  </Table.Tr>
                ))
              )}
            </Table.Tbody>
          </Table>
        </Table.ScrollContainer>
      </Stack>
    </Paper>
  );
}
