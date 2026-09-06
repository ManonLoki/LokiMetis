import { Group, SegmentedControl, Stack } from '@mantine/core';
import { Link } from '@tanstack/react-router';
import { useTranslation } from 'react-i18next';

import {
  selectDashboardClientOptions,
  selectDashboardWorkbuddyOption,
} from '../ai-capabilities';
import type {
  AgentClientKind,
  AvailableAiTypeDto,
  UsageViewKind,
} from '../api/usage-types';
import { LocalScanProgressBar } from './LocalScanProgressBar';

/** 描述用量区域页头横向菜单中的已批准页面。 */
interface UsagePageItem {
  /** 页面路由，不承载筛选或敏感信息。 */
  to:
    | '/dashboard'
    | '/dashboard/calls'
    | '/dashboard/usage'
    | '/dashboard/charts'
    | '/dashboard/sources'
    | '/dashboard/settings';
  /** 本地翻译资源中的页面键。 */
  key: 'overview' | 'calls' | 'statistics' | 'charts' | 'sources' | 'dashboardSettings';
}

/** 看板设置始终作为页头最后一项，紧挨数据源或「全部」视图最后一项。 */
const dashboardSettingsItem: UsagePageItem = {
  key: 'dashboardSettings',
  to: '/dashboard/settings',
};

/** 看板横向菜单。 */
const dashboardPageItems: UsagePageItem[] = [
  { key: 'overview', to: '/dashboard' },
  { key: 'statistics', to: '/dashboard/usage' },
  { key: 'charts', to: '/dashboard/charts' },
  { key: 'sources', to: '/dashboard/sources' },
  dashboardSettingsItem,
];

/** 全部视图只开放可跨 Agent 合并的只读概览与调用页，设置仍出现在最后。 */
const allDashboardPageItems: UsagePageItem[] = [
  { key: 'overview', to: '/dashboard' },
  { key: 'calls', to: '/dashboard/calls' },
  dashboardSettingsItem,
];

/** WorkBuddy 只读视图开放概览、用量与数据源，不提供调用表。 */
const workbuddyDashboardPageItems: UsagePageItem[] = [
  { key: 'overview', to: '/dashboard' },
  { key: 'statistics', to: '/dashboard/usage' },
  { key: 'charts', to: '/dashboard/charts' },
  { key: 'sources', to: '/dashboard/sources' },
  dashboardSettingsItem,
];

/** 定义公共页头的只读视图与横向子页交互。 */
interface DashboardToolbarProps {
  /** 后端统一目录中当前可映射到看板的 AI 类型。 */
  availableAiTypes: AvailableAiTypeDto[];
  /** 用户已开放、会出现在页头的客户端。 */
  enabledAgents: AgentClientKind[];
  /** 页头切换当前只读视图。 */
  onViewChange: (view: UsageViewKind) => void;
  /** 当前概览或调用视图。 */
  view: UsageViewKind;
  /** 用户是否已在设置中开放 WorkBuddy 本地统计；决定页头是否展示该入口。 */
  workbuddyStatsEnabled: boolean;
}

/** 把当前区域子页渲染为横向可访问链接。 */
function UsagePageLinks({ view }: { view: UsageViewKind }) {
  const { t } = useTranslation();
  const items =
    view === 'all'
      ? allDashboardPageItems
      : view === 'workbuddy'
        ? workbuddyDashboardPageItems
        : dashboardPageItems;
  return items.map((item) => {
    const label = t(`shell.navigation.${item.key}.label`);
    const description = t(`shell.navigation.${item.key}.description`);
    return (
      <Link
        activeOptions={{ exact: true }}
        activeProps={{ 'aria-current': 'page', className: 'navigation-link active' }}
        aria-label={t('shell.navigation.itemAria', { description, label })}
        className="navigation-link"
        key={item.to}
        to={item.to}
      >
        {label}
      </Link>
    );
  });
}

/** 用量区域顶部粘滞页头：已开放 Agent 切换与横向页面菜单。 */
export function DashboardToolbar({
  availableAiTypes,
  enabledAgents,
  onViewChange,
  view,
  workbuddyStatsEnabled,
}: DashboardToolbarProps) {
  const { t } = useTranslation();
  const switcherData = selectDashboardClientOptions(availableAiTypes).filter((item) =>
    enabledAgents.includes(item.value),
  );
  const workbuddyOption = selectDashboardWorkbuddyOption(availableAiTypes);
  const viewSwitcherData: Array<{ label: string; value: UsageViewKind }> = [
    { label: t('shell.clientAll'), value: 'all' },
    ...switcherData,
    ...(workbuddyStatsEnabled && workbuddyOption
      ? [{ label: workbuddyOption.name, value: 'workbuddy' as const }]
      : []),
  ];
  const showSwitcher =
    switcherData.length > 0 || (workbuddyStatsEnabled && workbuddyOption !== null);
  return (
    <Stack
      className="dashboard-toolbar"
      data-dashboard-toolbar=""
      gap={0}
      style={{ position: 'sticky', top: 0 }}
    >
      <Group
        align="center"
        className="dashboard-header-row"
        data-testid="dashboard-header-row"
        justify="space-between"
        wrap="nowrap"
      >
        <nav aria-label={t('shell.navigation.pagesAria')} className="dashboard-page-nav">
          <UsagePageLinks view={view} />
        </nav>
        {showSwitcher ? (
          <SegmentedControl
            aria-label={t('shell.clientSelectorAria')}
            data={viewSwitcherData}
            onChange={(value) => {
              const selected = viewSwitcherData.find((item) => item.value === value);
              if (selected) onViewChange(selected.value);
            }}
            size="xs"
            value={view}
          />
        ) : null}
      </Group>
      <LocalScanProgressBar enabledAgents={enabledAgents} />
    </Stack>
  );
}
