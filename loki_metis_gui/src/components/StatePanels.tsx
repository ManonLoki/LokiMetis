import {
  Alert,
  Badge,
  Button,
  Paper,
  SimpleGrid,
  Skeleton,
  Stack,
  Text,
  Title,
} from "@mantine/core";
import type { ReactNode } from "react";
import { useTranslation } from "react-i18next";

import type { UiMessageCode } from "../api/usage";
import { uiMessageLabel } from "../i18n/backend-labels";
import { visibleErrorMessage } from "../visible-error";

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

/** 表格或面板内的单行提示（空数据、更新中），与表头左对齐，不做居中装饰。 */
export function EmptyHint({
  children,
  color,
  compact = false,
}: {
  children: ReactNode;
  color?: string;
  compact?: boolean;
}) {
  return (
    <Text
      c={color ?? "dimmed"}
      p={compact ? undefined : "lg"}
      py={compact ? "md" : undefined}
    >
      {children}
    </Text>
  );
}
