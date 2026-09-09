import {
  Alert,
  Button,
  Checkbox,
  Code,
  Group,
  Modal,
  ScrollArea,
  Stack,
  Text,
  TextInput,
} from "@mantine/core";
import { IconAlertTriangle, IconCopy, IconSparkles } from "@tabler/icons-react";
import { useEffect, useMemo, useState, type ReactElement } from "react";
import { useTranslation } from "react-i18next";

import type {
  PreparedSkinImportBatch,
  SkinAppearanceCheck,
  SkinCreationPrompt,
} from "../../api/skins";

/** 描述通用确认弹窗中的本地化内容与确认回调。 */
export interface SkinConfirmDialogProps {
  blocked?: boolean;
  confirmColor?: string;
  description: string;
  opened: boolean;
  pending: boolean;
  title: string;
  onCancel: () => void;
  onConfirm: () => void;
}

/** 渲染删除、转换和定向重启共用的显式确认边界。 */
export function SkinConfirmDialog({
  blocked = false,
  confirmColor,
  description,
  opened,
  pending,
  title,
  onCancel,
  onConfirm,
}: SkinConfirmDialogProps): ReactElement {
  const { t } = useTranslation();
  return (
    <Modal onClose={onCancel} opened={opened} title={title}>
      <Stack>
        <Text>{description}</Text>
        <Group justify="flex-end">
          <Button disabled={pending} onClick={onCancel} variant="default">
            {t("skins.dialog.cancel")}
          </Button>
          <Button
            color={confirmColor}
            disabled={blocked}
            loading={pending}
            onClick={onConfirm}
          >
            {t("skins.dialog.confirm")}
          </Button>
        </Group>
      </Stack>
    </Modal>
  );
}

/** 描述外观不一致确认弹窗所需数据。 */
export interface AppearanceDialogProps {
  blocked?: boolean;
  check: SkinAppearanceCheck | null;
  pending: boolean;
  onCancel: () => void;
  onConfirm: () => void;
}

/** 展示宿主只读探测到的主题变量差异，并要求用户显式继续。 */
export function AppearanceDialog({
  blocked = false,
  check,
  pending,
  onCancel,
  onConfirm,
}: AppearanceDialogProps): ReactElement {
  const { t } = useTranslation();
  return (
    <Modal onClose={onCancel} opened={check !== null} title={t("skins.appearance.title")}>
      <Stack>
        <Alert color="yellow" icon={<IconAlertTriangle size={18} />}>
          {t("skins.appearance.description", { mode: check?.effectiveMode ?? "" })}
        </Alert>
        {check?.differences.map((difference) => (
          <Text key={difference.field} size="sm">
            <Text component="span" fw={650}>
              {difference.label}:{" "}
            </Text>
            {difference.currentValue ?? t("skins.appearance.unreadable")} →{" "}
            {difference.expectedValue}
          </Text>
        ))}
        <Group justify="flex-end">
          <Button disabled={pending} onClick={onCancel} variant="default">
            {t("skins.dialog.cancel")}
          </Button>
          <Button disabled={blocked} loading={pending} onClick={onConfirm}>
            {t("skins.appearance.continue")}
          </Button>
        </Group>
      </Stack>
    </Modal>
  );
}

/** 描述应用兼容皮肤前一次性的第三方代码信任确认。 */
export interface ThirdPartyCodeDialogProps {
  blocked?: boolean;
  hostName: string;
  opened: boolean;
  pending: boolean;
  skinName: string;
  onCancel: () => void;
  onConfirm: () => void;
}

/** 明确说明兼容脚本会被执行；结构校验不被表述为脚本安全证明。 */
export function ThirdPartyCodeDialog({
  blocked = false,
  hostName,
  opened,
  pending,
  skinName,
  onCancel,
  onConfirm,
}: ThirdPartyCodeDialogProps): ReactElement {
  const { t } = useTranslation();
  const [trusted, setTrusted] = useState(false);
  useEffect(() => {
    setTrusted(false);
  }, [opened, skinName]);
  return (
    <Modal onClose={onCancel} opened={opened} title={t("skins.code_trust.apply_title")}>
      <Stack>
        <Alert color="red" icon={<IconAlertTriangle size={18} />}>
          {t("skins.code_trust.apply_description", { host: hostName, name: skinName })}
        </Alert>
        <Text c="dimmed" size="sm">
          {t("skins.code_trust.not_safety_proof")}
        </Text>
        <Checkbox
          checked={trusted}
          disabled={blocked}
          label={t("skins.code_trust.apply_acknowledgement")}
          onChange={(event) => setTrusted(event.currentTarget.checked)}
        />
        <Group justify="flex-end">
          <Button disabled={pending} onClick={onCancel} variant="default">
            {t("skins.dialog.cancel")}
          </Button>
          <Button
            color="red"
            disabled={blocked || !trusted}
            loading={pending}
            onClick={onConfirm}
          >
            {t("skins.code_trust.continue")}
          </Button>
        </Group>
      </Stack>
    </Modal>
  );
}

/** 描述导入预检弹窗的批次和选中项。 */
export interface ImportDialogProps {
  batch: PreparedSkinImportBatch | null;
  blocked?: boolean;
  pending: boolean;
  selected: string[];
  onCancel: () => void;
  onCommit: (allowThirdPartyCode: boolean) => void;
  onSelectedChange: (selected: string[]) => void;
}

/** 展示原生层安全解压后的逐项结果，仅提交用户确认的项目。 */
export function ImportDialog({
  batch,
  blocked = false,
  pending,
  selected,
  onCancel,
  onCommit,
  onSelectedChange,
}: ImportDialogProps): ReactElement {
  const { t } = useTranslation();
  const selectedIds = useMemo(() => new Set(selected), [selected]);
  const selectedLegacyItems = useMemo(
    () =>
      batch?.items.filter(
        (item) => selectedIds.has(item.itemId) && item.skin.packageType === "legacySkin",
      ) ?? [],
    [batch?.items, selectedIds],
  );
  const selectedLegacyKey = useMemo(
    () => selectedLegacyItems.map((item) => item.itemId).join("\u0000"),
    [selectedLegacyItems],
  );
  const [thirdPartyCodeTrusted, setThirdPartyCodeTrusted] = useState(false);
  useEffect(() => {
    setThirdPartyCodeTrusted(false);
  }, [batch?.token, selectedLegacyKey]);
  const trustRequired = selectedLegacyItems.length > 0;
  return (
    <Modal
      onClose={onCancel}
      opened={batch !== null}
      size="lg"
      title={t("skins.import.title")}
    >
      <Stack>
        <Text c="dimmed" size="sm">
          {t("skins.import.summary", {
            ready: batch?.items.length ?? 0,
            total: batch?.totalFiles ?? 0,
          })}
        </Text>
        {trustRequired ? (
          <Alert
            color="red"
            icon={<IconAlertTriangle size={18} />}
            title={t("skins.code_trust.import_title")}
          >
            <Stack gap="xs">
              <Text size="sm">
                {t("skins.code_trust.import_description", {
                  count: selectedLegacyItems.length,
                })}
              </Text>
              <Text size="sm">{t("skins.code_trust.not_safety_proof")}</Text>
              <Checkbox
                checked={thirdPartyCodeTrusted}
                disabled={blocked}
                label={t("skins.code_trust.import_acknowledgement")}
                onChange={(event) => setThirdPartyCodeTrusted(event.currentTarget.checked)}
              />
            </Stack>
          </Alert>
        ) : null}
        <ScrollArea.Autosize mah={320}>
          <Stack gap="xs">
            {batch?.items.map((item) => (
              <Checkbox
                checked={selected.includes(item.itemId)}
                description={item.archiveName}
                disabled={blocked}
                key={item.itemId}
                label={`${item.skin.name} · ${item.skin.author}`}
                onChange={(event) => {
                  onSelectedChange(
                    event.currentTarget.checked
                      ? [...selected, item.itemId]
                      : selected.filter((id) => id !== item.itemId),
                  );
                }}
              />
            ))}
            {batch?.skipped.map((item) => (
              <Alert
                color="yellow"
                key={`${item.archiveName}-${item.code}`}
                title={item.archiveName}
              >
                {item.message}
              </Alert>
            ))}
          </Stack>
        </ScrollArea.Autosize>
        <Group justify="flex-end">
          <Button disabled={pending} onClick={onCancel} variant="default">
            {t("skins.dialog.cancel")}
          </Button>
          <Button
            disabled={
              blocked || selected.length === 0 || (trustRequired && !thirdPartyCodeTrusted)
            }
            loading={pending}
            onClick={() => onCommit(trustRequired && thirdPartyCodeTrusted)}
          >
            {t("skins.import.commit", { count: selected.length })}
          </Button>
        </Group>
      </Stack>
    </Modal>
  );
}

/** 描述创建主题与获取 Codex 生成提示词的双入口。 */
export interface CreateThemeDialogProps {
  blocked?: boolean;
  opened: boolean;
  pending: boolean;
  prompt: SkinCreationPrompt | null;
  promptPending: boolean;
  onClose: () => void;
  onCreate: (name: string, author: string) => void;
  onLoadPrompt: () => void;
}

/** 渲染快速主题脚手架表单及可复制的动态 Codex 提示词。 */
export function CreateThemeDialog({
  blocked = false,
  opened,
  pending,
  prompt,
  promptPending,
  onClose,
  onCreate,
  onLoadPrompt,
}: CreateThemeDialogProps): ReactElement {
  const { t } = useTranslation();
  const [name, setName] = useState("");
  const [author, setAuthor] = useState("");
  useEffect(() => {
    if (!opened) {
      setName("");
      setAuthor("");
    }
  }, [opened]);
  return (
    <Modal onClose={onClose} opened={opened} size="lg" title={t("skins.create.title")}>
      <Stack>
        <Text c="dimmed" size="sm">
          {t("skins.create.description")}
        </Text>
        <TextInput
          disabled={blocked}
          label={t("skins.create.name")}
          maxLength={80}
          onChange={(event) => setName(event.currentTarget.value)}
          value={name}
        />
        <TextInput
          disabled={blocked}
          label={t("skins.create.author")}
          maxLength={80}
          onChange={(event) => setAuthor(event.currentTarget.value)}
          value={author}
        />
        <Group>
          <Button
            disabled={blocked || !name.trim() || !author.trim()}
            loading={pending}
            onClick={() => onCreate(name, author)}
          >
            {t("skins.create.scaffold")}
          </Button>
          <Button
            disabled={blocked}
            leftSection={<IconSparkles size={17} />}
            loading={promptPending}
            onClick={onLoadPrompt}
            variant="light"
          >
            {t("skins.create.codex_prompt")}
          </Button>
        </Group>
        {prompt !== null ? (
          <Stack gap="xs">
            <Group justify="space-between">
              <Text fw={650} size="sm">
                {t("skins.create.prompt_title")}
              </Text>
              <Button
                leftSection={<IconCopy size={16} />}
                onClick={() => void navigator.clipboard.writeText(prompt.prompt)}
                size="xs"
                variant="subtle"
              >
                {t("skins.create.copy")}
              </Button>
            </Group>
            <ScrollArea.Autosize mah={260}>
              <Code block>{prompt.prompt}</Code>
            </ScrollArea.Autosize>
          </Stack>
        ) : null}
      </Stack>
    </Modal>
  );
}
