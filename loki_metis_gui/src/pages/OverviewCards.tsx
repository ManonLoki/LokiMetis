import { Badge, Group, Paper, SimpleGrid, Stack, Text, Title } from "@mantine/core";
import { useTranslation } from "react-i18next";

import type { WindowUsageDto } from "../api/usage";
import { FactMeta, TokenTotalDisplay } from "../components/UsageUi";
import { formatBasisPoints, formatTokens } from "../usage-format";

/** 渲染所选本机窗口的主要指标、完整 Token 构成和多根覆盖。 */
export function LocalWindowSummary({ windowUsage }: { windowUsage: WindowUsageDto }) {
  const { t } = useTranslation();
  const aggregate = windowUsage.fact.value;
  const label = t(`window.${windowUsage.window}`);
  const uncachedInputTokens =
    aggregate.tokens.cachedInputTokens === null ||
    aggregate.tokens.inputTokens < aggregate.tokens.cachedInputTokens
      ? null
      : aggregate.tokens.inputTokens - aggregate.tokens.cachedInputTokens;
  const tokenMetrics = [
    { label: t("metric.input"), value: aggregate.tokens.inputTokens },
    { label: t("metric.cachedInput"), value: aggregate.tokens.cachedInputTokens },
    { label: t("metric.cacheWrite"), value: aggregate.tokens.cacheWriteInputTokens },
    { label: t("metric.uncachedInput"), value: uncachedInputTokens },
    { label: t("metric.output"), value: aggregate.tokens.outputTokens },
    { label: t("metric.reasoningOutput"), value: aggregate.tokens.reasoningOutputTokens },
  ];
  const coverageMetrics = [
    {
      label: t("metric.cacheReadShare"),
      value:
        aggregate.tokens.cachedInputTokens === null
          ? t("common.notProvided")
          : formatBasisPoints(aggregate.cacheReadBasisPoints),
    },
    {
      label: t("overviewCards.local.cachedReadCalls"),
      value: formatTokens(aggregate.cachedReadCallCount),
    },
    { label: t("overviewCards.local.threads"), value: formatTokens(aggregate.threadCount) },
    { label: t("overviewCards.local.roots"), value: formatTokens(aggregate.rootCount) },
    { label: t("overviewCards.local.sources"), value: formatTokens(aggregate.sourceCount) },
    {
      label: t("overviewCards.local.duplicates"),
      value: formatTokens(aggregate.crossRootDuplicateSourceCount),
    },
  ];
  return (
    <section aria-label={t("overviewCards.local.sectionAria", { window: label })}>
      <Stack gap="md">
        <SimpleGrid cols={{ base: 1, sm: 2 }}>
          <Paper
            className="metric-card local-card local-hero-card"
            p="lg"
            radius="lg"
            withBorder
          >
            <Text c="dimmed" fw={700} size="sm">
              {t("overviewCards.local.callCount")}
            </Text>
            <Text className="hero-number" fw={800}>
              {formatTokens(aggregate.callCount)}
            </Text>
          </Paper>
          <Paper
            className="metric-card local-card local-hero-card"
            p="lg"
            radius="lg"
            withBorder
          >
            <Text c="dimmed" fw={700} size="sm">
              {t("overviewCards.local.tokenTotal")}
            </Text>
            <TokenTotalDisplay
              className="hero-number"
              value={aggregate.tokens.totalTokens}
            />
          </Paper>
        </SimpleGrid>

        <Paper className="breakdown-panel" p="lg" radius="lg" withBorder>
          <Stack gap="md">
            <Group justify="space-between">
              <Title order={3}>
                {t("overviewCards.local.breakdownTitle", { window: label })}
              </Title>
              <FactMeta fact={windowUsage.fact} />
            </Group>
            <SimpleGrid cols={{ base: 2, sm: 3, xl: 6 }}>
              {tokenMetrics.map((metric) => (
                <div className="mini-metric" key={metric.label}>
                  <Text c="dimmed" size="xs">
                    {metric.label}
                  </Text>
                  <TokenTotalDisplay density="inline" value={metric.value} />
                </div>
              ))}
            </SimpleGrid>
          </Stack>
        </Paper>

        <Paper className="breakdown-panel" p="lg" radius="lg" withBorder>
          <Stack gap="md">
            <Group justify="space-between">
              <Title order={3}>{t("overviewCards.local.coverageTitle")}</Title>
              <Badge color="yellow" variant="light">
                {t("overviewCards.local.enabledRootsBadge")}
              </Badge>
            </Group>
            <SimpleGrid cols={{ base: 2, sm: 3, xl: 6 }}>
              {coverageMetrics.map((metric) => (
                <div className="mini-metric" key={metric.label}>
                  <Text c="dimmed" size="xs">
                    {metric.label}
                  </Text>
                  <Text fw={800}>{metric.value}</Text>
                </div>
              ))}
            </SimpleGrid>
          </Stack>
        </Paper>
      </Stack>
    </section>
  );
}
