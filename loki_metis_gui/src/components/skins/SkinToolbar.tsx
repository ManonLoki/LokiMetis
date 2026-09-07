import { Button, Group, Select, Switch, TextInput } from "@mantine/core";
import { IconPlus, IconRefresh, IconSearch, IconUpload } from "@tabler/icons-react";
import type { ReactElement } from "react";
import { useTranslation } from "react-i18next";

import type { CodexInstance } from "../../api/skins";

/** 描述皮肤资源库工具栏的受控状态。 */
export interface SkinToolbarProps {
  busy: boolean;
  hostAvailable: boolean;
  instances: CodexInstance[];
  search: string;
  selectedInstanceId: string | null;
  userOnly: boolean;
  onCreate: () => void;
  onImport: () => void;
  onRefresh: () => void;
  onSearchChange: (value: string) => void;
  onSelectedInstanceChange: (value: string | null) => void;
  onUserOnlyChange: (value: boolean) => void;
}

/** 渲染搜索、用户筛选、显式实例选择和资源库操作。 */
export function SkinToolbar({
  busy,
  hostAvailable,
  instances,
  search,
  selectedInstanceId,
  userOnly,
  onCreate,
  onImport,
  onRefresh,
  onSearchChange,
  onSelectedInstanceChange,
  onUserOnlyChange,
}: SkinToolbarProps): ReactElement {
  const { t } = useTranslation();
  return (
    <Group align="end" gap="sm" justify="space-between">
      <Group align="end" gap="sm" style={{ flex: "1 1 520px" }}>
        <TextInput
          aria-label={t("skins.search.label")}
          leftSection={<IconSearch aria-hidden="true" size={17} />}
          onChange={(event) => onSearchChange(event.currentTarget.value)}
          placeholder={t("skins.search.placeholder")}
          style={{ flex: "1 1 220px" }}
          value={search}
        />
        <Select
          aria-label={t("skins.instance.label")}
          clearable={instances.length !== 1}
          data={instances.map((instance) => ({
            label: instance.label,
            value: instance.id,
          }))}
          disabled={!hostAvailable || instances.length === 0}
          onChange={onSelectedInstanceChange}
          placeholder={t(
            instances.length === 0 ? "skins.instance.none" : "skins.instance.choose",
          )}
          style={{ flex: "1 1 230px" }}
          value={selectedInstanceId}
        />
        <Switch
          checked={userOnly}
          label={t("skins.search.user_only")}
          onChange={(event) => onUserOnlyChange(event.currentTarget.checked)}
        />
      </Group>
      <Group gap="xs">
        <Button
          disabled={!hostAvailable || busy}
          leftSection={<IconUpload size={17} />}
          onClick={onImport}
          variant="default"
        >
          {t("skins.action.import")}
        </Button>
        <Button
          disabled={!hostAvailable || busy}
          leftSection={<IconPlus size={17} />}
          onClick={onCreate}
          variant="default"
        >
          {t("skins.action.create")}
        </Button>
        <Button
          disabled={!hostAvailable || busy}
          leftSection={<IconRefresh size={17} />}
          onClick={onRefresh}
          variant="light"
        >
          {t("skins.action.refresh")}
        </Button>
      </Group>
    </Group>
  );
}
