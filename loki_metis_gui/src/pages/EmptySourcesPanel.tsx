import { Alert } from "@mantine/core";
import { useTranslation } from "react-i18next";

/** 定义没有可用数据根时的说明与跳转操作。 */
interface EmptySourcesPanelProps {
  clientLabel: string;
}

/** 数据根为空时只提示尚未发现，不展开扫描规则长文。 */
export function EmptySourcesPanel({ clientLabel }: EmptySourcesPanelProps) {
  const { t } = useTranslation();
  return <Alert color="orange" title={t("sources.empty.title", { client: clientLabel })} />;
}
