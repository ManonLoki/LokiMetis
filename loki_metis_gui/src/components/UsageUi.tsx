import { Check, Copy } from "@phosphor-icons/react";
import {
  ActionIcon,
  Alert,
  Badge,
  Button,
  CopyButton,
  Group,
  SimpleGrid,
  Skeleton,
  Paper,
  Stack,
  Text,
  Title,
  Tooltip,
} from "@mantine/core";
import { Link } from "@tanstack/react-router";
import { useTranslation } from "react-i18next";

import type { LocalIndexState, MetricFactDto, UiMessageCode } from "../api/usage";
import { uiMessageLabel } from "../i18n/backend-labels";
import { CountUp } from "./motion/CountUp";
import {
  completenessLabel,
  confidenceLabel,
  formatCompactTokens,
  formatTokens,
  freshnessLabel,
  providerLabel,
} from "../usage-format";
import { visibleErrorMessage } from "../visible-error";

// 本文件是各业务页面（Overview/Usage/Calls/Sources）共用的一组小型
// “纯展示”组件（只接收 props 渲染 UI，不发起网络请求、不持有复杂状态），
// 类似后端 GUI 里的 DTO——把常见的展示模式（加载中/失败/空状态/事实
// 质量标签等）抽出来复用，页面组件本身只需要关心业务数据怎么获取。

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

/** 在 Tauri 仍返回中性状态时展示真实实施阶段，不填充采集数字。 */
export function ImplementationState({
  message,
  messageCode,
}: {
  message: string | null;
  messageCode?: UiMessageCode | null;
}) {
  const { i18n, t } = useTranslation();
  // 三级优先级：有稳定消息代码（messageCode）就用它查双语翻译，最可靠；
  // 没有代码但有中文原始消息且当前就是中文界面，直接展示原始消息
  // （历史兼容路径，旧版本后端可能只给中文文案没给代码）；
  // 都不满足（比如英文界面下只有中文原始消息）时，回退成固定的通用提示，
  // 避免在英文界面里意外混入一段中文文本。
  const visibleMessage = messageCode
    ? uiMessageLabel(t, messageCode)
    : i18n.resolvedLanguage === "zh-CN" && message
      ? message
      : t("ui.implementation.fallback");
  return (
    <Paper className="state-panel" radius="lg" withBorder>
      <Stack align="center" gap="sm">
        <Badge color="yellow" size="lg" variant="light">
          {t("ui.implementation.badge")}
        </Badge>
        <Title order={2}>{t("ui.implementation.title")}</Title>
        <Text c="dimmed" maw={620} ta="center">
          {visibleMessage}
        </Text>
      </Stack>
    </Paper>
  );
}

/** 提供页面级异步加载状态并向读屏器播报。 */
export function LoadingState({ label }: { label?: string }) {
  const { t } = useTranslation();
  const visibleLabel = label ?? t("ui.loadingDefault");
  return (
    <Stack aria-busy="true" aria-live="polite" data-testid="loading-skeleton" gap="lg">
      <span className="sr-only">{t("ui.loadingVisible", { label: visibleLabel })}</span>
      <Skeleton height={44} radius="lg" />
      <SimpleGrid cols={{ base: 1, sm: 2, lg: 4 }}>
        {Array.from({ length: 4 }, (_, index) => (
          <Skeleton height={112} key={index} radius="lg" />
        ))}
      </SimpleGrid>
      <Skeleton height={280} radius="lg" />
    </Stack>
  );
}

/** 展示脱敏错误与显式重试入口，不暴露内部堆栈。 */
export function FailureState({
  error,
  fallback,
  onRetry,
}: {
  error: unknown;
  fallback?: string;
  onRetry: () => void;
}) {
  const { t } = useTranslation();
  const detail = visibleErrorMessage(error, fallback);
  return (
    <Alert color="red" title={t("ui.failureTitle")} variant="light">
      <Stack gap="sm">
        <Text size="sm">{detail}</Text>
        <Button onClick={onRetry} variant="light">
          {t("common.retry")}
        </Button>
      </Stack>
    </Alert>
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
