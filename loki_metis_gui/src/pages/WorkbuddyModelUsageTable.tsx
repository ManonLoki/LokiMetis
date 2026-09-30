import { Badge, Group, Paper, Stack, Table, Text } from "@mantine/core";
import { useTranslation } from "react-i18next";

import type { WorkbuddyModelUsageWindowDto } from "../api/usage";
import { TokenTotalDisplay } from "../components/usage/UsageUi";
import { formatCredits, formatTokens } from "../usage-format";
import { EmptyHint } from "../components/StatePanels";
import { EndTh, EndTd } from "../components/DataTable";

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
      aria-label={t("workbuddy.modelUsage.title")}
      className="table-panel"
      data-testid="workbuddy-model-usage"
      radius="lg"
      withBorder
    >
      <Stack gap={0}>
        <Stack gap="xs" p="lg">
          <Group justify="space-between">
            <Text fw={700}>{t("workbuddy.modelUsage.title")}</Text>
            <Badge color="brand" variant="light">
              {t("workbuddy.modelUsage.jsonlScope")}
            </Badge>
          </Group>
        </Stack>

        <Table.ScrollContainer minWidth={1320}>
          <Table className="data-table" verticalSpacing="sm">
            <Table.Thead>
              <Table.Tr>
                <Table.Th>{t("workbuddy.modelUsage.model")}</Table.Th>
                <EndTh>{t("metric.totalTokens")}</EndTh>
                <EndTh>{t("metric.input")}</EndTh>
                <EndTh>{t("metric.cachedInput")}</EndTh>
                <EndTh>{t("metric.uncachedInput")}</EndTh>
                <EndTh>{t("metric.output")}</EndTh>
                <EndTh>{t("workbuddy.modelUsage.requests")}</EndTh>
                <EndTh>{t("workbuddy.modelUsage.topLevelCalls")}</EndTh>
                <EndTh>{t("workbuddy.modelUsage.subagentCalls")}</EndTh>
                <EndTh>{t("workbuddy.totalCredits")}</EndTh>
              </Table.Tr>
            </Table.Thead>
            <Table.Tbody>
              {groups.length === 0 ? (
                <Table.Tr>
                  <Table.Td colSpan={10}>
                    <EmptyHint compact>{t("workbuddy.modelUsage.empty")}</EmptyHint>
                  </Table.Td>
                </Table.Tr>
              ) : (
                groups.map((group, index) => (
                  <Table.Tr key={group.model ?? `unattributed-${index}`}>
                    <Table.Td fw={700}>
                      {group.model ?? t("workbuddy.modelUsage.unattributed")}
                    </Table.Td>
                    <EndTd>
                      <TokenTotalDisplay density="inline" value={group.totalTokens} />
                    </EndTd>
                    <EndTd>
                      <TokenTotalDisplay density="inline" value={group.inputTokens} />
                    </EndTd>
                    <EndTd>
                      <TokenTotalDisplay density="inline" value={group.cachedInputTokens} />
                    </EndTd>
                    <EndTd>
                      <TokenTotalDisplay
                        density="inline"
                        value={group.uncachedInputTokens}
                      />
                    </EndTd>
                    <EndTd>
                      <TokenTotalDisplay density="inline" value={group.outputTokens} />
                    </EndTd>
                    <EndTd>{formatTokens(group.callCount)}</EndTd>
                    <EndTd>{formatTokens(group.topLevelCallCount)}</EndTd>
                    <EndTd>{formatTokens(group.subagentCallCount)}</EndTd>
                    <EndTd>{formatCredits(group.credits)}</EndTd>
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
