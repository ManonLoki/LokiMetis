import { Alert, Badge, Checkbox, Group, Paper, SimpleGrid, Stack, Title } from '@mantine/core';
import { useMutation, useQueryClient } from '@tanstack/react-query';
import { useAtomValue } from 'jotai';
import { useTranslation } from 'react-i18next';

import { setEnabledAgents, setWorkbuddyStatsEnabled, type AgentClientKind } from '../api/usage';
import { synchronizeGlobalPrivacySettings } from '../api/usage-queries';
import { agentClientAtom, visibleUsageClients } from '../state/agent-client';
import { visibleErrorMessage } from '../visible-error';

/** 定义设置页 Agent 多选的当前值、保存状态与错误。 */
interface EnabledAgentsSettingsProps {
  /** 当前已开放的本机 Agent。 */
  savedAgents: AgentClientKind[];
  /** 当前是否已显式开放读取 WorkBuddy 本地用量统计。 */
  savedWorkbuddyStatsEnabled: boolean;
}

/** 逐项打开或关闭监控和上报，立即反映到页头选项卡；WorkBuddy 开启后计入全部合计与上报。 */
export function EnabledAgentsSettings({
  savedAgents,
  savedWorkbuddyStatsEnabled,
}: EnabledAgentsSettingsProps) {
  const { t } = useTranslation();
  const client = useAtomValue(agentClientAtom);
  const queryClient = useQueryClient();
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
          {visibleUsageClients.map((item) => (
            <Checkbox
              checked={savedAgents.includes(item.value)}
              disabled={mutation.isPending}
              key={item.value}
              label={item.label}
              onChange={(event) => {
                const next = event.currentTarget.checked
                  ? [...savedAgents, item.value]
                  : savedAgents.filter((agent) => agent !== item.value);
                mutation.mutate(next);
              }}
            />
          ))}
          <Checkbox
            checked={savedWorkbuddyStatsEnabled}
            disabled={workbuddyMutation.isPending}
            label={t('privacy.enabledAgents.workbuddyLabel')}
            onChange={(event) => {
              workbuddyMutation.mutate(event.currentTarget.checked);
            }}
          />
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
