import { Badge, Button, Group, Paper, Select, Stack, Title } from '@mantine/core';
import { useTranslation } from 'react-i18next';

import type { SourceRootDto } from '../api/usage';

/** 保留 Codex 官方上下文所需的唯一主数据根选择。 */
export function SourcePrimaryRootControls({
  disabled,
  onChange,
  pending,
  roots,
}: {
  disabled: boolean;
  onChange: (rootId: string | null) => void;
  pending: boolean;
  roots: SourceRootDto[];
}) {
  const { t } = useTranslation();
  return (
    <Paper className="scan-card" p="lg" radius="lg" withBorder>
      <Stack gap="md">
        <div>
          <Group gap="xs">
            <Title order={3}>{t('sources.primary.title')}</Title>
            <Badge color="red" variant="light">
              {t('sources.primary.badge')}
            </Badge>
          </Group>
        </div>
        <Select
          clearable
          data={roots
            .filter((root) => root.enabled)
            .map((root, _index, enabledRoots) => ({
              label:
                enabledRoots.filter((candidate) => candidate.alias === root.alias).length > 1
                  ? t('sources.primary.duplicate', { alias: root.alias, id: root.id.slice(-8) })
                  : root.alias,
              value: root.id,
            }))}
          disabled={disabled}
          label={t('sources.primary.label')}
          loading={pending}
          nothingFoundMessage={t('sources.primary.empty')}
          onChange={onChange}
          placeholder={t('sources.primary.placeholder')}
          value={roots.find((root) => root.isPrimary)?.id ?? null}
        />
        <Group justify="flex-end">
          <Button
            disabled={!roots.some((root) => root.isPrimary) || disabled}
            loading={pending}
            onClick={() => onChange(null)}
            size="compact-sm"
            variant="subtle"
          >
            {t('sources.primary.clear')}
          </Button>
        </Group>
      </Stack>
    </Paper>
  );
}
