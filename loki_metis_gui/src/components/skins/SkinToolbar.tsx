import { Button, Group, Switch, TextInput } from "@mantine/core";
import { IconPlus, IconRefresh, IconSearch, IconUpload } from "@tabler/icons-react";
import type { ReactElement } from "react";
import { useTranslation } from "react-i18next";

/** 描述皮肤资源库工具栏的受控状态。 */
export interface SkinToolbarProps {
  busy: boolean;
  hostAvailable: boolean;
  search: string;
  userOnly: boolean;
  onCreate: () => void;
  onImport: () => void;
  onRefresh: () => void;
  onSearchChange: (value: string) => void;
  onUserOnlyChange: (value: boolean) => void;
}

/** 渲染搜索、用户筛选和资源库操作；宿主目标只按唯一实例自动解析。 */
export function SkinToolbar({
  busy,
  hostAvailable,
  search,
  userOnly,
  onCreate,
  onImport,
  onRefresh,
  onSearchChange,
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
