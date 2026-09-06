import { Stack } from "@mantine/core";
import { Link } from "@tanstack/react-router";
import { useTranslation } from "react-i18next";

/** 监控区横向二级选项卡。 */
const monitorPageItems = [
  { key: "workbench", to: "/monitor" },
  { key: "management", to: "/monitor/management" },
  { key: "images", to: "/monitor/images" },
  { key: "settings", to: "/monitor/settings" },
] as const;

/** 监控区粘滞页头：工作台、监控管理、图片管理、Hooks 设置。 */
export function MonitorToolbar() {
  const { t } = useTranslation();
  return (
    <Stack className="monitor-toolbar" data-monitor-toolbar="" gap={0}>
      <nav aria-label={t("monitor.navigation.pagesAria")} className="monitor-page-nav">
        {monitorPageItems.map((item) => {
          const label = t(`monitor.navigation.${item.key}.label`);
          const description = t(`monitor.navigation.${item.key}.description`);
          return (
            <Link
              activeOptions={{ exact: true }}
              activeProps={{
                "aria-current": "page",
                className: "monitor-navigation-link active",
              }}
              aria-label={t("monitor.navigation.itemAria", { description, label })}
              className="monitor-navigation-link"
              key={item.to}
              to={item.to}
            >
              {label}
            </Link>
          );
        })}
      </nav>
    </Stack>
  );
}
