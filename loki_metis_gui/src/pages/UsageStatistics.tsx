import {
  Badge,
  Group,
  NativeSelect,
  Paper,
  SegmentedControl,
  SimpleGrid,
  Stack,
  Table,
  Text,
} from "@mantine/core";
import { useTranslation } from "react-i18next";

import type {
  TimeStandard,
  UsageDimension,
  UsageStatisticsDto,
  UsageWindow,
} from "../api/usage";
import { FilterBar } from "../components/FilterBar";
import { TokenTotalDisplay, MiniMetric } from "../components/usage/UsageUi";
import {
  completenessLabel,
  confidenceLabel,
  formatBasisPoints,
  formatCalendarDate,
  formatObservedAt,
  formatTokens,
} from "../usage-format";
import { displayLabel } from "../i18n/backend-labels";
import { usageWindowOrder } from "../lib/overview-windows";
import { EmptyHint } from "../components/StatePanels";
import { EndTh, EndTd } from "../components/DataTable";

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

const dimensionValues: UsageDimension[] = [
  "model",
  "reasoningEffort",
  "project",
  "thread",
  "root",
];

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
  const windowOptions = usageWindowOrder.map((value) => ({
    label: t(`window.${value}`),
    value,
  }));
  const dimensionOptions = dimensionValues.map((value) => ({
    label: t(`dimension.${value}`),
    value,
  }));

  return (
    <Stack gap="lg">
      <FilterBar
        label={t("statistics.controls.aria")}
        status={
          <>
            <Badge
              color={statistics.fact.completeness === "complete" ? "teal" : "yellow"}
              variant="light"
            >
              {completenessLabel(statistics.fact.completeness)}
            </Badge>
            <Badge
              color={statistics.fact.confidence === "exact" ? "teal" : "yellow"}
              variant="light"
            >
              {confidenceLabel(statistics.fact.confidence)}
            </Badge>
            <Text c="dimmed" size="xs">
              {fetching
                ? t("statistics.controls.updating")
                : t("statistics.controls.observed", {
                    date: formatObservedAt(statistics.observedAtEpochMs),
                  })}
            </Text>
          </>
        }
      >
        <SegmentedControl
          aria-label={t("statistics.controls.window")}
          data={windowOptions}
          onChange={(value) => onWindowChange(value as UsageWindow)}
          value={window}
        />
        <NativeSelect
          aria-label={t("statistics.controls.dimension")}
          className="w-64"
          data={dimensionOptions.filter(
            (option) => reasoningAvailable || option.value !== "reasoningEffort",
          )}
          leftSection={
            <Text c="dimmed" size="xs">
              {t("statistics.controls.dimension")}
            </Text>
          }
          leftSectionWidth={76}
          leftSectionPointerEvents="none"
          onChange={(event) =>
            onDimensionChange(event.currentTarget.value as UsageDimension)
          }
          value={dimension}
        />
      </FilterBar>

      <SimpleGrid cols={{ base: 1, sm: 2, lg: 4 }}>
        <MiniMetric label={t("statistics.summary.totalTokens")}>
          <TokenTotalDisplay
            className="window-number"
            value={aggregate.tokens.totalTokens}
          />
        </MiniMetric>
        <MiniMetric label={t("statistics.summary.calls")}>
          <Text className="window-number" fw={800}>
            {formatTokens(aggregate.callCount)}
          </Text>
        </MiniMetric>
        <MiniMetric label={t("statistics.summary.threads")}>
          <Text className="window-number" fw={800}>
            {formatTokens(aggregate.threadCount)}
          </Text>
        </MiniMetric>
        <MiniMetric label={t("statistics.summary.cacheReadShare")}>
          <Text className="window-number" fw={800}>
            {aggregate.tokens.cachedInputTokens === null
              ? t("common.notProvided")
              : formatBasisPoints(aggregate.cacheReadBasisPoints)}
          </Text>
        </MiniMetric>
      </SimpleGrid>

      <Paper className="table-panel" radius="lg" withBorder>
        <Stack gap={0}>
          <Group justify="space-between" p="lg">
            <Text fw={700}>{t("statistics.daily.title")}</Text>
          </Group>
          <Table.ScrollContainer minWidth={760}>
            <Table className="data-table" verticalSpacing="sm">
              <Table.Thead>
                <Table.Tr>
                  <Table.Th>
                    {timeStandard.mode === "custom"
                      ? t("statistics.daily.dateRemote")
                      : t("statistics.daily.dateLocal")}
                  </Table.Th>
                  <EndTh>{t("metric.totalTokens")}</EndTh>
                  <EndTh>{t("metric.input")}</EndTh>
                  <EndTh>{t("metric.cachedInput")}</EndTh>
                  <EndTh>{t("metric.output")}</EndTh>
                  <EndTh>{t("metric.calls")}</EndTh>
                </Table.Tr>
              </Table.Thead>
              <Table.Tbody>
                {statistics.dailyBuckets.map((bucket) => (
                  <Table.Tr key={bucket.localDate}>
                    <Table.Td>
                      <Group gap="xs" wrap="nowrap">
                        <Text>{formatCalendarDate(bucket.localDate)}</Text>
                        {bucket.inProgress ? (
                          <Badge color="yellow" size="xs" variant="light">
                            {t("statistics.daily.inProgress")}
                          </Badge>
                        ) : null}
                      </Group>
                    </Table.Td>
                    <EndTd fw={700}>
                      <TokenTotalDisplay
                        density="inline"
                        value={bucket.measure.tokens.totalTokens}
                      />
                    </EndTd>
                    <EndTd>
                      <TokenTotalDisplay
                        density="inline"
                        value={bucket.measure.tokens.inputTokens}
                      />
                    </EndTd>
                    <EndTd>
                      <TokenTotalDisplay
                        density="inline"
                        value={bucket.measure.tokens.cachedInputTokens}
                      />
                    </EndTd>
                    <EndTd>
                      <TokenTotalDisplay
                        density="inline"
                        value={bucket.measure.tokens.outputTokens}
                      />
                    </EndTd>
                    <EndTd>{formatTokens(bucket.measure.callCount)}</EndTd>
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
            <Text fw={700}>{t("statistics.groups.title")}</Text>
          </Group>
          <Table.ScrollContainer minWidth={760}>
            <Table className="data-table" highlightOnHover verticalSpacing="sm">
              <Table.Thead>
                <Table.Tr>
                  <Table.Th>
                    {t(`dimension.${dimension}`, { defaultValue: t("dimension.group") })}
                  </Table.Th>
                  <EndTh>{t("metric.totalTokens")}</EndTh>
                  <EndTh>{t("statistics.groups.tokenShare")}</EndTh>
                  <EndTh>{t("metric.calls")}</EndTh>
                  <EndTh>{t("metric.cacheReadShare")}</EndTh>
                  <Table.Th>{t("statistics.groups.quality")}</Table.Th>
                </Table.Tr>
              </Table.Thead>
              <Table.Tbody>
                {groupRows.map((group) => (
                  <Table.Tr key={group.id}>
                    <Table.Td>
                      <Group gap="xs">
                        <Text fw={group.remainder ? 700 : 500}>
                          {displayLabel(
                            t,
                            group.label,
                            group.labelCode,
                            group.disambiguationIndex,
                          )}
                        </Text>
                        {group.remainder ? (
                          <Badge color="gray" size="xs" variant="light">
                            {t("statistics.groups.merged")}
                          </Badge>
                        ) : null}
                      </Group>
                    </Table.Td>
                    <EndTd fw={700}>
                      <TokenTotalDisplay
                        density="inline"
                        value={group.measure.tokens.totalTokens}
                      />
                    </EndTd>
                    <EndTd>{formatBasisPoints(group.totalTokenShareBasisPoints)}</EndTd>
                    <EndTd>{formatTokens(group.measure.callCount)}</EndTd>
                    <EndTd>
                      {group.measure.tokens.cachedInputTokens === null
                        ? t("common.notProvided")
                        : formatBasisPoints(group.measure.cacheReadBasisPoints)}
                    </EndTd>
                    <Table.Td>{confidenceLabel(group.measure.confidence)}</Table.Td>
                  </Table.Tr>
                ))}
              </Table.Tbody>
            </Table>
          </Table.ScrollContainer>
          {groupRows.length === 0 ? (
            <EmptyHint>{t("statistics.groups.empty")}</EmptyHint>
          ) : null}
        </Stack>
      </Paper>
    </Stack>
  );
}
