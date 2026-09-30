import { ActionIcon, Button, TextInput } from "@mantine/core";
import {
  Plus,
  ArrowsClockwise,
  MagnifyingGlass,
  UploadSimple,
} from "@phosphor-icons/react";
import type { ReactElement } from "react";
import { useTranslation } from "react-i18next";

import { Magnetic } from "../motion/Magnetic";

/** 描述皮肤资源库工具栏的受控状态。 */
export interface SkinToolbarProps {
  busy: boolean;
  hostAvailable: boolean;
  search: string;
  onCreate: () => void;
  onImport: () => void;
  onRefresh: () => void;
  onSearchChange: (value: string) => void;
}

/** 渲染搜索和资源库操作；宿主目标只按唯一实例自动解析。 */
export function SkinToolbar({
  busy,
  hostAvailable,
  search,
  onCreate,
  onImport,
  onRefresh,
  onSearchChange,
}: SkinToolbarProps): ReactElement {
  const { t } = useTranslation();
  return (
    <div className="flex min-w-0 flex-1 items-center justify-end gap-2">
      <TextInput
        aria-label={t("skins.search.label")}
        className="w-full max-w-80"
        leftSection={<MagnifyingGlass aria-hidden="true" size={16} />}
        onChange={(event) => onSearchChange(event.currentTarget.value)}
        placeholder={t("skins.search.placeholder")}
        size="sm"
        value={search}
      />
      <Magnetic>
        <Button
          disabled={!hostAvailable || busy}
          leftSection={<UploadSimple size={16} />}
          onClick={onImport}
          size="sm"
          variant="default"
        >
          {t("skins.action.import")}
        </Button>
      </Magnetic>
      <Magnetic>
        <Button
          disabled={!hostAvailable || busy}
          leftSection={<Plus size={16} />}
          onClick={onCreate}
          size="sm"
        >
          {t("skins.action.create")}
        </Button>
      </Magnetic>
      <ActionIcon
        aria-label={t("skins.action.refresh")}
        disabled={!hostAvailable || busy}
        onClick={onRefresh}
        size={36}
        variant="default"
      >
        <ArrowsClockwise aria-hidden="true" size={18} />
      </ActionIcon>
    </div>
  );
}
