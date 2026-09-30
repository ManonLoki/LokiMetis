import { Check, Copy } from "@phosphor-icons/react";
import {
  ActionIcon,
  Alert,
  Badge,
  Button,
  CopyButton,
  Group,
  Paper,
  Stack,
  Text,
  Tooltip,
} from "@mantine/core";
import { Link } from "@tanstack/react-router";
import type { ReactNode } from "react";
import { useTranslation } from "react-i18next";

import type { LocalIndexState, MetricFactDto } from "../../api/usage";
import { CountUp } from "../motion/CountUp";
import {
  completenessLabel,
  confidenceLabel,
  formatCompactTokens,
  formatTokens,
  freshnessLabel,
  providerLabel,
} from "../../usage-format";

// 本文件是看板与 WorkBuddy 各页面共用的“纯展示”组件：Token 合计、索引状态说明、
// 事实质量标签与小指标卡；只接收 props 渲染，不发起请求、不持有复杂状态。

/** 在任意 Token 表面同时展示两位小数 K/M/B 与千分位精确整数；空值保持未提供。 */
export function TokenTotalDisplay({
  className,
  density = "hero",
  value,
}: {
  className?: string;
  density?: "hero" | "inline";
  value: number | null;
}) {
  const { t } = useTranslation();
  if (value === null) {
    return (
      <Text className={className} c="dimmed">
        {t("common.notProvided")}
      </Text>
    );
  }
  const exactValue = formatTokens(value);
  if (density === "inline") {
    return (
      <Stack align="flex-end" className="token-inline" gap={0}>
        <Text className={className} fw={700}>
          <CountUp format={formatCompactTokens} value={value} />
        </Text>
        <Text className="exact-token-number" c="dimmed" size="xs">
          {exactValue}
        </Text>
      </Stack>
    );
  }
  return (
    <Stack gap={0}>
      <Text className={className} fw={800}>
        <CountUp format={formatCompactTokens} value={value} />
      </Text>
      <Group gap={4} wrap="nowrap">
        <Text className="exact-token-number" c="dimmed" size="xs">
          {t("ui.tokenExact", { value: exactValue })}
        </Text>
        <CopyButton timeout={1500} value={exactValue}>
          {({ copied, copy }) => (
            <Tooltip
              label={copied ? t("ui.tokenExactCopied") : t("ui.tokenExactCopy")}
              withArrow
            >
              <ActionIcon
                aria-label={t("ui.tokenExactCopy")}
                color={copied ? "teal" : "gray"}
                onClick={copy}
                size="sm"
                variant="subtle"
              >
                {copied ? (
                  <Check aria-hidden="true" size={14} />
                ) : (
                  <Copy aria-hidden="true" size={14} />
                )}
              </ActionIcon>
            </Tooltip>
          )}
        </CopyButton>
      </Group>
    </Stack>
  );
}

/** 根据后端索引四态解释空统计，并只引导用户进入显式扫描页面。 */
export function LocalIndexNotice({ state }: { state: LocalIndexState }) {
  const { t } = useTranslation();
  if (state === "ready") {
    return null;
  }
  const notice = {
    action: t(`ui.localIndex.${state}.action`),
    detail: t(`ui.localIndex.${state}.body`),
    title: t(`ui.localIndex.${state}.title`),
  };

  return (
    <Alert
      color={state === "readyNoCalls" ? "blue" : "orange"}
      title={notice.title}
      variant="light"
    >
      <Stack align="flex-start" gap="sm">
        <Text size="sm">{notice.detail}</Text>
        <Button component={Link} to="/dashboard/sources" variant="light">
          {notice.action}
        </Button>
      </Stack>
    </Alert>
  );
}

/** 展示事实的 provider、时效、覆盖与置信度，支持每个数字追溯。 */
// `<T>` 泛型组件：MetricFactDto 对应后端 core 的 MetricFact<T>（同一个
// "信封"包装不同种类的业务值），这个组件只关心信封上的元信息标签
// （provider/freshness/completeness/confidence），完全不关心 T 具体是
// 本机窗口还是统计分组，所以可以直接对任意 MetricFactDto<T> 复用。
export function FactMeta<T>({ fact }: { fact: MetricFactDto<T> }) {
  useTranslation();
  return (
    <Group gap="xs">
      <Badge color="yellow" variant="light">
        {providerLabel(fact.provider)}
      </Badge>
      <Badge color="gray" variant="light">
        {freshnessLabel(fact.freshness)}
      </Badge>
      <Badge color={fact.completeness === "complete" ? "green" : "orange"} variant="light">
        {completenessLabel(fact.completeness)}
      </Badge>
      <Badge color={fact.confidence === "exact" ? "blue" : "orange"} variant="light">
        {confidenceLabel(fact.confidence)}
      </Badge>
    </Group>
  );
}

/** 统计页与图表页共用的小指标卡：标签在上、数值在下；`dense` 用于面板内的紧凑网格。 */
export function MiniMetric({
  children,
  dense = false,
  label,
}: {
  children: ReactNode;
  dense?: boolean;
  label: string;
}) {
  const caption = (
    <Text c="dimmed" size={dense ? "xs" : "sm"}>
      {label}
    </Text>
  );
  if (dense) {
    return (
      <div className="mini-metric">
        {caption}
        {children}
      </div>
    );
  }
  return (
    <Paper className="mini-metric" p="lg" radius="lg" withBorder>
      {caption}
      {children}
    </Paper>
  );
}
