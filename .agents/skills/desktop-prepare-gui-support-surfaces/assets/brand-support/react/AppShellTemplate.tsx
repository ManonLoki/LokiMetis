import { AppShell as MantineAppShell } from "@mantine/core";
import { useState, type ReactElement, type ReactNode } from "react";

import {
  APP_SIDEBAR_LOGO_PATH,
  AppSidebarTemplate,
  detailedSidebarNavbarWidth,
  persistDetailedSidebarCollapsed,
  readDetailedSidebarCollapsed,
  type FeatureNavigationItem,
} from "./AppSidebarTemplate";
import { type SupportPageSelection } from "./supportNavigation";

/** 已批准 detailed 运行时壳层所需的展示输入。 */
export interface AppShellTemplateProps {
  activePath: string;
  applicationName: string;
  children: ReactNode;
  featureItems: FeatureNavigationItem[];
  onNavigate: (path: string) => void;
  supportPages: SupportPageSelection;
  version: string;
}

/**
 * 建立固定 detailed AppShell，并让主内容偏移与侧栏折叠状态同步。
 * compact 产品不得接入此运行时模板，应按其 profile 使用固定 compact 壳层。
 */
export function AppShellTemplate({
  activePath,
  applicationName,
  children,
  featureItems,
  onNavigate,
  supportPages,
  version,
}: AppShellTemplateProps): ReactElement {
  const [detailedSidebarCollapsed, setDetailedSidebarCollapsed] = useState(
    readDetailedSidebarCollapsed,
  );
  const navbarWidth = detailedSidebarNavbarWidth(detailedSidebarCollapsed);

  /** 同步壳层状态与独立设备偏好，不把折叠状态写入页面会话 store。 */
  const handleCollapsedChange = (nextCollapsed: boolean): void => {
    setDetailedSidebarCollapsed(nextCollapsed);
    persistDetailedSidebarCollapsed(nextCollapsed);
  };

  return (
    <MantineAppShell
      data-mode="detailed"
      data-navbar-width={navbarWidth}
      data-testid="app-shell"
      navbar={{ width: navbarWidth }}
    >
      <MantineAppShell.Navbar p={0}>
        <AppSidebarTemplate
          activePath={activePath}
          applicationName={applicationName}
          detailedCollapsed={detailedSidebarCollapsed}
          featureItems={featureItems}
          logoSrc={APP_SIDEBAR_LOGO_PATH}
          mode="detailed"
          onCollapsedChange={handleCollapsedChange}
          onNavigate={onNavigate}
          supportPages={supportPages}
          version={version}
        />
      </MantineAppShell.Navbar>
      <MantineAppShell.Main>{children}</MantineAppShell.Main>
    </MantineAppShell>
  );
}
