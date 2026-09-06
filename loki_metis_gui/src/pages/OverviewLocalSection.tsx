import { SegmentedControl, Stack, Title } from "@mantine/core";
import { useTranslation } from "react-i18next";

import type { LocalRecordsSectionDto, UsageWindow, WindowUsageDto } from "../api/usage";
import { LocalIndexNotice } from "../components/UsageUi";
import { LocalWindowSummary } from "./OverviewCards";
import { overviewWindowOrder } from "./overview-windows";

/** 定义物理 Agent 本机概览区域的规范事实与加载状态。 */
interface OverviewLocalSectionProps {
  /** 已通过窗口完整性校验的本机记录区块。 */
  local: LocalRecordsSectionDto;
  /** 当前选中的本机窗口事实。 */
  selectedUsage: WindowUsageDto;
  /** 当前选中的窗口枚举。 */
  selectedWindow: UsageWindow;
  onWindowChange: (window: UsageWindow) => void;
}

/** 渲染本机记录窗口、索引提示与 Token 摘要。 */
export function OverviewLocalSection({
  local,
  onWindowChange,
  selectedUsage,
  selectedWindow,
}: OverviewLocalSectionProps) {
  const { t } = useTranslation();
  return (
    <section aria-labelledby="local-heading">
      <Stack gap="md">
        <div>
          <Title id="local-heading" order={2}>
            {t("overview.local.title")}
          </Title>
        </div>

        <LocalIndexNotice state={local.indexState} />

        {local.indexState === "notScanned" || local.indexState === "needsRescan" ? null : (
          <>
            <SegmentedControl
              aria-label={t("overview.local.windowAria")}
              data={overviewWindowOrder.map((window) => ({
                label: t(`window.${window}`),
                value: window,
              }))}
              onChange={(value) => onWindowChange(value as UsageWindow)}
              value={selectedWindow}
            />
            <LocalWindowSummary windowUsage={selectedUsage} />
          </>
        )}
      </Stack>
    </section>
  );
}
