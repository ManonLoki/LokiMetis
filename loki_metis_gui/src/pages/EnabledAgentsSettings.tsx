import {
  Alert,
  Badge,
  Checkbox,
  Group,
  Paper,
  SimpleGrid,
  Stack,
  Title,
} from '@mantine/core';
import { useMutation, useQueryClient } from '@tanstack/react-query';
import { useAtomValue } from 'jotai';
import { useTranslation } from 'react-i18next';

import {
  setEnabledAgents,
  setWorkbuddyStatsEnabled,
  type AgentClientKind,
  type AvailableAiTypeDto,
} from '../api/usage';
import { synchronizeGlobalPrivacySettings } from '../api/usage-queries';
import {
  selectDashboardClientOptions,
  selectDashboardWorkbuddyOption,
  selectEnabledDashboardClients,
} from '../ai-capabilities';
import { agentClientAtom } from '../state/agent-client';
import { visibleErrorMessage } from '../visible-error';

/** 定义设置页 Agent 多选的当前值、保存状态与错误。 */
interface EnabledAgentsSettingsProps {
  /** 后端统一目录中当前可映射到看板的 AI 类型。 */
  availableAiTypes: AvailableAiTypeDto[];
  /** 当前已开放的本机 Agent。 */
  savedAgents: AgentClientKind[];
  /** 当前是否已显式开放读取 WorkBuddy 本地用量统计。 */
  savedWorkbuddyStatsEnabled: boolean;
}

/** 逐项打开或关闭监控和上报，立即反映到页头选项卡；WorkBuddy 开启后计入全部合计与上报。 */
export function EnabledAgentsSettings({
  availableAiTypes,
  savedAgents,
  savedWorkbuddyStatsEnabled,
}: EnabledAgentsSettingsProps) {
  const { t } = useTranslation();
  const client = useAtomValue(agentClientAtom);
  const queryClient = useQueryClient();
  const clientOptions = selectDashboardClientOptions(availableAiTypes);
  const workbuddyOption = selectDashboardWorkbuddyOption(availableAiTypes);
  const enabledAgents = selectEnabledDashboardClients(savedAgents, availableAiTypes);
  const mutation = useMutation({
    mutationFn: (agents: AgentClientKind[]) => setEnabledAgents(client, agents),
    onSuccess: (settings) => {
      queryClient.setQueryData(['privacy-settings', client], settings);
      synchronizeGlobalPrivacySettings(queryClient, settings);
    },
  });
  const workbuddyMutation = useMutation({
    mutationFn: (enabled: boolean) => setWorkbuddyStatsEnabled(client, enabled),
    onSuccess: (settings) => {
      queryClient.setQueryData(['privacy-settings', client], settings);
      synchronizeGlobalPrivacySettings(queryClient, settings);
    },
  });
  return (
    <Paper className="privacy-card" p="lg" radius="lg" withBorder>
      <Stack gap="sm">
        <Group gap="xs">
          <Title id="dashboard-enabled-agents-title" order={3}>
            {t('privacy.enabledAgents.title')}
          </Title>
          <Badge color="blue" variant="light">
            {t('privacy.enabledAgents.badge')}
          </Badge>
        </Group>
        <SimpleGrid
          aria-labelledby="dashboard-enabled-agents-title"
          cols={{ base: 2, sm: 3, md: 4 }}
          data-testid="dashboard-enabled-agent-options"
          role="group"
          spacing="xs"
          verticalSpacing="xs"
        >
          {clientOptions.map((item) => (
            <Checkbox
              checked={enabledAgents.includes(item.value)}
              disabled={mutation.isPending}
              key={item.value}
              label={item.label}
              onChange={(event) => {
                const selected = new Set(enabledAgents);
                if (event.currentTarget.checked) selected.add(item.value);
                else selected.delete(item.value);
                mutation.mutate(
                  clientOptions
                    .map((option) => option.value)
                    .filter((agent) => selected.has(agent)),
                );
              }}
            />
          ))}
          {workbuddyOption ? (
            <Checkbox
              checked={savedWorkbuddyStatsEnabled}
              disabled={workbuddyMutation.isPending}
              label={workbuddyOption.name}
              onChange={(event) => {
                workbuddyMutation.mutate(event.currentTarget.checked);
              }}
            />
          ) : null}
        </SimpleGrid>
        {mutation.isError || workbuddyMutation.isError ? (
          <Alert color="red" title={t('ui.failureTitle')}>
            {visibleErrorMessage(mutation.error ?? workbuddyMutation.error)}
          </Alert>
        ) : null}
      </Stack>
    </Paper>
  );
}
