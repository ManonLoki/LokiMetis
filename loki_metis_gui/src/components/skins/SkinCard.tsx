import {
  ActionIcon,
  Badge,
  Button,
  Card,
  Checkbox,
  Group,
  Image,
  Menu,
  Stack,
  Text,
} from "@mantine/core";
import {
  IconDots,
  IconDownload,
  IconFolderOpen,
  IconRefresh,
  IconTrash,
} from "@tabler/icons-react";
import { memo, type ReactElement } from "react";
import { useTranslation } from "react-i18next";

import type { SkinDescriptor } from "../../api/skins";

/** 描述资源卡片的只读身份和受控操作。 */
export interface SkinCardProps {
  active: boolean;
  busy: boolean;
  selected: boolean;
  skin: SkinDescriptor;
  onApply: (skin: SkinDescriptor) => void;
  onConvert: (skin: SkinDescriptor) => void;
  onDelete: (skin: SkinDescriptor) => void;
  onExport: (skin: SkinDescriptor) => void;
  onOpen: (skin: SkinDescriptor) => void;
  onSelect: (skin: SkinDescriptor, selected: boolean) => void;
  onStop: () => void;
}

/** 渲染带预览、格式标签、主操作和用户资源菜单的单张皮肤卡。 */
function SkinCardImpl({
  active,
  busy,
  selected,
  skin,
  onApply,
  onConvert,
  onDelete,
  onExport,
  onOpen,
  onSelect,
  onStop,
}: SkinCardProps): ReactElement {
  const { t } = useTranslation();
  const userSkin = skin.source === "user";
  return (
    <Card
      className="surface-card"
      data-testid={`skin-card-${skin.id}`}
      p={0}
      radius="lg"
      withBorder
    >
      <Card.Section pos="relative">
        <Image
          alt={t("skins.card.preview_alt", { name: skin.name })}
          fallbackSrc="data:image/svg+xml,%3Csvg xmlns='http://www.w3.org/2000/svg' width='800' height='450'%3E%3Crect width='100%25' height='100%25' fill='%23808b9a'/%3E%3C/svg%3E"
          h={176}
          loading="lazy"
          src={skin.previewDataUrl}
        />
        {userSkin ? (
          <Checkbox
            aria-label={t("skins.card.select", { name: skin.name })}
            checked={selected}
            onChange={(event) => onSelect(skin, event.currentTarget.checked)}
            pos="absolute"
            right={12}
            top={12}
          />
        ) : null}
      </Card.Section>
      <Stack gap="sm" p="md">
        <Group align="flex-start" justify="space-between" wrap="nowrap">
          <Stack gap={2} style={{ minWidth: 0 }}>
            <Text fw={700} lineClamp={1}>
              {skin.name}
            </Text>
            <Text c="dimmed" lineClamp={1} size="xs">
              {t("skins.card.by", { author: skin.author })} · {skin.version}
            </Text>
          </Stack>
          <Badge color={skin.packageType === "theme" ? "violet" : "gray"} variant="light">
            {t(`skins.package.${skin.packageType}`)}
          </Badge>
        </Group>
        <Group gap={6}>
          <Badge color={skin.source === "builtin" ? "blue" : "teal"} variant="dot">
            {t(`skins.source.${skin.source}`)}
          </Badge>
          {skin.packageType === "legacySkin" ? (
            <Badge color="red" variant="light">
              {t("skins.card.third_party_code")}
            </Badge>
          ) : null}
          {skin.supportedColorModes.map((mode) => (
            <Badge key={mode} variant="outline">
              {t(`skins.mode.${mode}`)}
            </Badge>
          ))}
        </Group>
        <Group gap="xs" justify="space-between" wrap="nowrap">
          <Button
            color={active ? "red" : "violet"}
            disabled={busy}
            fullWidth
            loading={busy}
            onClick={() => (active ? onStop() : onApply(skin))}
            variant={active ? "light" : "filled"}
          >
            {active ? t("skins.action.stop") : t("skins.action.apply")}
          </Button>
          {userSkin ? (
            <Menu position="bottom-end" shadow="md" withinPortal>
              <Menu.Target>
                <ActionIcon
                  aria-label={t("skins.card.actions", { name: skin.name })}
                  disabled={busy}
                  size="lg"
                  variant="default"
                >
                  <IconDots aria-hidden="true" size={18} />
                </ActionIcon>
              </Menu.Target>
              <Menu.Dropdown>
                <Menu.Item
                  leftSection={<IconFolderOpen size={16} />}
                  onClick={() => onOpen(skin)}
                >
                  {t("skins.action.open")}
                </Menu.Item>
                <Menu.Item
                  leftSection={<IconDownload size={16} />}
                  onClick={() => onExport(skin)}
                >
                  {t("skins.action.export")}
                </Menu.Item>
                {skin.packageType === "legacySkin" ? (
                  <Menu.Item
                    leftSection={<IconRefresh size={16} />}
                    onClick={() => onConvert(skin)}
                  >
                    {t("skins.action.convert")}
                  </Menu.Item>
                ) : null}
                <Menu.Divider />
                <Menu.Item
                  color="red"
                  leftSection={<IconTrash size={16} />}
                  onClick={() => onDelete(skin)}
                >
                  {t("skins.action.delete")}
                </Menu.Item>
              </Menu.Dropdown>
            </Menu>
          ) : null}
        </Group>
      </Stack>
    </Card>
  );
}

export const SkinCard = memo(SkinCardImpl);
